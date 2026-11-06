/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Internal mutex interface.

use std::collections::HashMap;
use std::num::NonZeroU32;

use super::{Environment, ThreadId};
use crate::libc::errno::{EBUSY, EDEADLK, EPERM};

/// Stores and manages mutexes. Note that all the methods for locking and
/// unlocking mutexes are on [Environment] instead, because they interact with
/// threads.
#[derive(Default)]
pub struct MutexState {
    // TODO?: Maybe this should be a Vec instead? It would be bad if there were
    // many mutexes over the lifetime of an application, but it would perform
    // better. Maybe it could also be a fixed size allocator? (although that
    // seems a little overkill)
    mutexes: HashMap<MutexId, Mutex>,
    // Hopefully there will never be more than 2^64 mutexes in an application's
    // lifetime :P
    mutex_count: u64,
    // Only the Windows Dead Space close path may enable this handoff.
    // Normal execution and other profiles retain their existing scheduler.
    exit_handoff: bool,
    exit_trace: bool,
    exit_trace_releases: u32,
}

/// Unique identifier for mutexes, used for mutexes held by host objects and
/// guest pthread mutexes.
pub type MutexId = u64;

struct Mutex {
    type_: MutexType,
    waiting_count: u32,
    /// The `NonZeroU32` is the number of locks on this thread (if it's a
    /// recursive mutex).
    locked: Option<(ThreadId, NonZeroU32)>,
}

#[repr(i32)]
#[derive(Debug, PartialEq, Copy, Clone)]
#[allow(non_camel_case_types)]
pub enum MutexType {
    PTHREAD_MUTEX_NORMAL = 0,
    PTHREAD_MUTEX_ERRORCHECK = 1,
    PTHREAD_MUTEX_RECURSIVE = 2,
}

impl TryFrom<i32> for MutexType {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MutexType::PTHREAD_MUTEX_NORMAL),
            1 => Ok(MutexType::PTHREAD_MUTEX_ERRORCHECK),
            2 => Ok(MutexType::PTHREAD_MUTEX_RECURSIVE),
            _ => Err("Value is not a valid mutex type!"),
        }
    }
}
pub const PTHREAD_MUTEX_DEFAULT: MutexType = MutexType::PTHREAD_MUTEX_NORMAL;

impl MutexState {
    /// Initializes a mutex and returns a handle to it. Similar to
    /// `pthread_mutex_init`, but for host code.
    pub fn init_mutex(&mut self, mutex_type: MutexType) -> MutexId {
        let mutex_id = self.mutex_count;
        self.mutex_count = self.mutex_count.checked_add(1).unwrap();
        self.mutexes.insert(
            mutex_id,
            Mutex {
                type_: mutex_type,
                waiting_count: 0,
                locked: None,
            },
        );
        log_dbg!("Created mutex #{}, type {:?}", mutex_id, mutex_type);
        mutex_id
    }

    /// Destroys a mutex and returns an error on failure (as errno). Similar to
    /// `pthread_mutex_destroy`, but for host code. Note that the mutex is not
    /// destroyed on an Err return.
    pub fn destroy_mutex(&mut self, mutex_id: MutexId) -> Result<(), i32> {
        let mutex = self.mutexes.get_mut(&mutex_id).unwrap();
        if mutex.locked.is_some() {
            log_dbg!("Attempted to destroy currently locked mutex, returning EBUSY!");
            return Err(EBUSY);
        } else if mutex.waiting_count != 0 {
            log_dbg!("Attempted to destroy mutex with waiting locks, returning EBUSY!");
            return Err(EBUSY);
        }
        // TODO?: If we switch to a vec-based system, we should reuse destroyed
        // ids if they are at the top of the stack.
        self.mutexes.remove(&mutex_id);
        Ok(())
    }

    pub fn mutex_is_locked(&self, mutex_id: MutexId) -> bool {
        self.mutexes
            .get(&mutex_id)
            .is_some_and(|mutex| mutex.locked.is_some())
    }

    pub fn mutex_is_recursive(&self, mutex_id: MutexId) -> bool {
        self.mutexes
            .get(&mutex_id)
            .is_some_and(|mutex| mutex.type_ == MutexType::PTHREAD_MUTEX_RECURSIVE)
    }

    pub fn mutex_is_locked_by(&self, mutex_id: MutexId, thread: ThreadId) -> bool {
        self.mutexes.get(&mutex_id).is_some_and(|mutex| {
            if let Some((lock_thread, _)) = mutex.locked {
                lock_thread == thread
            } else {
                false
            }
        })
    }

    pub(crate) fn trace_lock(&self, mutex_id: MutexId) -> Option<(MutexType, u32, u32)> {
        self.mutexes.get(&mutex_id).map(|mutex| (mutex.type_,
            mutex.locked.map_or(0, |(_, count)| count.get()), mutex.waiting_count))
    }
}

impl Environment {
    /// Prevent cooperative mutex starvation during the native close
    /// callbacks. No callback is bypassed and no mutex ownership is forged.
    pub(crate) fn begin_pc_exit_mutex_handoff(&mut self) {
        if !cfg!(target_os = "windows") || self.bundle.bundle_identifier() != "com.ea.deadspace.bv" {
            return;
        }
        self.mutex_state.exit_handoff =
            std::env::var("TOUCHHLE_DS_EXIT_MUTEX_HANDOFF").as_deref() != Ok("0");
        self.mutex_state.exit_trace = crate::ds_exit_trace::configured();
        if self.mutex_state.exit_trace {
            echo!("[DSexit] mutex_handoff={}", self.mutex_state.exit_handoff);
        }
    }

    /// Relock mutex that was just unblocked. This should probably only be used
    /// by the thread scheduler.
    pub fn relock_unblocked_mutex_for_thread(&mut self, thread_id: ThreadId, mutex_id: MutexId) {
        log_dbg!(
            "Relocking unblocked mutex {} for thread {}, waiting count {}",
            mutex_id,
            self.current_thread,
            self.mutex_state
                .mutexes
                .get_mut(&mutex_id)
                .unwrap()
                .waiting_count
        );
        let mutex: &mut _ = self.mutex_state.mutexes.get_mut(&mutex_id).unwrap();
        assert!(mutex.locked.is_none());
        mutex.locked = Some((thread_id, NonZeroU32::new(1).unwrap()));
        if self
            .mutex_state
            .mutexes
            .get_mut(&mutex_id)
            .unwrap()
            .waiting_count
            > 0
        {
            self.mutex_state
                .mutexes
                .get_mut(&mutex_id)
                .unwrap()
                .waiting_count -= 1;
        }
    }

    /// Locks a mutex and returns the lock count or an error (as errno). Similar
    /// to `pthread_mutex_lock`, but for host code.
    pub fn lock_mutex(&mut self, mutex_id: MutexId) -> Result<u32, i32> {
        let current_thread = self.current_thread;
        let mutex: &mut _ = self.mutex_state.mutexes.get_mut(&mutex_id).unwrap();

        let Some((locking_thread, lock_count)) = mutex.locked else {
            log_dbg!("Locked mutex #{} for thread {}.", mutex_id, current_thread);
            mutex.locked = Some((current_thread, NonZeroU32::new(1).unwrap()));
            return Ok(1);
        };

        if locking_thread == current_thread {
            match mutex.type_ {
                MutexType::PTHREAD_MUTEX_NORMAL => {
                    // POSIX says behaviour is undefined here; on real iOS the
                    // guest deadlocks. We can't deadlock a single host thread
                    // safely, and panicking tears the whole emulator down for
                    // a guest bug. Treat it like an error-checking mutex and
                    // surface EDEADLK so the guest at least has a chance to
                    // notice rather than corrupting host state.
                    log!(
                        "Warning: pthread_mutex_lock: non-error-checking mutex #{mutex_id} would deadlock on thread {current_thread}; returning EDEADLK instead of panicking the host.",
                    );
                    return Err(EDEADLK);
                }
                MutexType::PTHREAD_MUTEX_ERRORCHECK => {
                    log_dbg!("Attempted to lock error-checking mutex #{} for thread {}, already locked by same thread! Returning EDEADLK.", mutex_id, current_thread);
                    return Err(EDEADLK);
                }
                MutexType::PTHREAD_MUTEX_RECURSIVE => {
                    log_dbg!(
                        "Increasing lock level on recursive mutex #{}, currently locked by thread {}.",
                        mutex_id,
                        locking_thread,
                    );
                    mutex.locked = Some((locking_thread, lock_count.checked_add(1).unwrap()));
                    return Ok(lock_count.get() + 1);
                }
            }
        }

        // Add to the waiting count, so that the mutex isn't destroyed. This is
        // subtracted in relock_unblocked_mutex.
        mutex.waiting_count += 1;

        // Mutex is already locked, block thread until it isn't.
        self.block_on_mutex(mutex_id);
        // Lock count is always 1 after a thread-blocking lock.
        Ok(1)
    }

    /// Unlocks a mutex and returns the lock count or an error (as errno).
    /// Similar to `pthread_mutex_unlock`, but for host code.
    pub fn unlock_mutex(&mut self, mutex_id: MutexId) -> Result<u32, i32> {
        let current_thread = self.current_thread;
        let mutex: &mut _ = self.mutex_state.mutexes.get_mut(&mutex_id).unwrap();

        let Some((locking_thread, lock_count)) = mutex.locked else {
            match mutex.type_ {
                MutexType::PTHREAD_MUTEX_NORMAL => {
                    // Убираем panic!, так как реальные iOS-игры часто пытаются
                    // разблокировать уже разблокированные мьютексы.
                    log_dbg!(
                        "Warning: Attempted to unlock non-error-checking mutex #{mutex_id} for thread {current_thread}, already unlocked! Ignoring and returning EPERM.",
                    );
                    return Err(EPERM);
                }
                MutexType::PTHREAD_MUTEX_ERRORCHECK | MutexType::PTHREAD_MUTEX_RECURSIVE => {
                    log_dbg!(
                        "Attempted to unlock error-checking or recursive mutex #{} for thread {}, already unlocked! Returning EPERM.",
                        mutex_id, current_thread,
                    );
                    return Err(EPERM);
                }
            }
        };

        if locking_thread != current_thread {
            match mutex.type_ {
                MutexType::PTHREAD_MUTEX_NORMAL => {
                    // This case is undefined,
                    // but tests on macOS/iOS shows it is allowed!
                    log!(
                        "Warning: Allowing to unlock non-error-checking mutex #{mutex_id} for thread {current_thread}, locked by different thread {locking_thread}!",
                    );
                }
                MutexType::PTHREAD_MUTEX_ERRORCHECK | MutexType::PTHREAD_MUTEX_RECURSIVE => {
                    log_dbg!(
                        "Attempted to unlock error-checking or recursive mutex #{} for thread {}, locked by different thread {}! Returning EPERM.",
                        mutex_id, current_thread, locking_thread,
                    );
                    return Err(EPERM);
                }
            }
        }

        if lock_count.get() == 1 {
            log_dbg!(
                "Unlocked mutex #{} for thread {}.",
                mutex_id,
                current_thread
            );
            mutex.locked = None;
            let contended = mutex.waiting_count != 0;
            if contended && self.mutex_state.exit_trace {
                self.mutex_state.exit_trace_releases += 1;
                if self.mutex_state.exit_trace_releases <= 8 {
                    echo!("[DSexit] mutex_released id={} owner={} lr={:#010x} release={} handoff={}",
                        mutex_id, current_thread, self.cpu.regs()[crate::cpu::Cpu::LR],
                        self.mutex_state.exit_trace_releases, self.mutex_state.exit_handoff);
                }
            }
            if contended && self.mutex_state.exit_handoff {
                // Hand the scheduling opportunity to the existing round-robin
                // scheduler only after the final recursive unlock. It still
                // selects the waiter and performs the normal relock bookkeeping.
                self.yield_thread(super::ThreadBlock::NotBlocked);
            }
            Ok(0)
        } else {
            assert!(mutex.type_ == MutexType::PTHREAD_MUTEX_RECURSIVE);
            log_dbg!(
                "Decreasing lock level on recursive mutex #{} (count {}), currently locked by thread {}.",
                mutex_id,
                lock_count,
                locking_thread
            );
            mutex.locked = Some((
                locking_thread,
                NonZeroU32::new(lock_count.get() - 1).unwrap(),
            ));
            Ok(lock_count.get() - 1)
        }
    }
}

#[cfg(test)]
mod exit_handoff_tests {
    use super::*;
    use crate::environment::{nullable_box::NullableBox, Thread, ThreadBlock};
    use corosensei::{Coroutine, CoroutineResult};
    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // A headless scheduler fixture with real core state. OpenALManager itself
    // opens no audio context/device; no app files, host window or guest runs.
    fn environment() -> Environment {
        let mem = crate::mem::Mem::new();
        let cpu = crate::cpu::Cpu::new(None);
        let threads = (0..2).map(|index| Thread {
            active: true, blocked_by: ThreadBlock::NotBlocked, return_value: None,
            guest_context: (index == 0).then(|| Box::new(crate::cpu::CpuContext::new())),
            host_context: None, stack: None, thread_local_framework_state: Default::default(),
        }).collect();
        Environment {
            startup_time: std::time::Instant::now(),
            bundle: NullableBox::new(crate::bundle::Bundle::new_fake_bundle()),
            fs: NullableBox::new(crate::fs::Fs::new_fake_fs()), window: None,
            openal_manager: NullableBox::new(crate::audio::openal::OpenALManager::new().unwrap()),
            mem: NullableBox::new(mem), bins: Vec::new(),
            objc: NullableBox::new(crate::objc::ObjC::new()),
            dyld: NullableBox::new(crate::dyld::Dyld::new()), cpu: NullableBox::new(cpu),
            current_thread: 1, threads, libc_state: Default::default(),
            mutex_state: Default::default(), framework_state: Default::default(),
            options: NullableBox::new(crate::options::Options::default()), gdb_server: None,
            env_vars: Default::default(), dump_file: None, is_app_picker: true,
            yielder: std::ptr::null(), remaining_ticks: None,
            panic_cell: std::rc::Rc::new(std::cell::Cell::new(None)),
            udf_bypass_last: None, udf_bypass_count: 0,
        }
    }

    fn waiting(env: &mut Environment, id: MutexId) {
        env.threads[0].blocked_by = ThreadBlock::Mutex(id);
        env.mutex_state.mutexes.get_mut(&id).unwrap().waiting_count += 1;
    }

    #[test]
    fn final_recursive_unlock_yields_then_scheduler_relocks_for_waiter() {
        let _serial = TEST_MUTEX.lock().unwrap();
        let mut env = environment();
        let id = env.mutex_state.init_mutex(MutexType::PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(env.lock_mutex(id), Ok(1));
        assert_eq!(env.lock_mutex(id), Ok(2));
        waiting(&mut env, id);
        env.mutex_state.exit_handoff = true;
        let mut worker = Coroutine::new(move |yielder, mut env: Environment| {
            env.with_yielder(yielder, move |env| {
                // A recursive decrement must not wake a waiter or yield.
                assert_eq!(env.unlock_mutex(id), Ok(1));
                assert!(env.mutex_state.mutex_is_locked_by(id, 1));
                assert_eq!(env.unlock_mutex(id), Ok(0));
            });
            env
        });
        let CoroutineResult::Yield(mut env) = worker.resume(env) else { panic!("final unlock did not yield") };
        assert!(!env.mutex_state.mutex_is_locked(id));
        assert_eq!(env.schedule_next_thread(), 0);
        assert!(env.mutex_state.mutex_is_locked_by(id, 0));
        assert_eq!(env.mutex_state.trace_lock(id), Some((MutexType::PTHREAD_MUTEX_RECURSIVE, 1, 0)));
        assert!(matches!(env.threads[0].blocked_by, ThreadBlock::NotBlocked));
        env.current_thread = 0;
        assert_eq!(env.unlock_mutex(id), Ok(0)); // No remaining waiter: no yield.
        env.current_thread = 1;
        assert!(matches!(worker.resume(env), CoroutineResult::Return(_)));
    }

    #[test]
    fn disabled_handoff_preserves_immediate_reacquisition() {
        let _serial = TEST_MUTEX.lock().unwrap();
        let mut env = environment();
        let id = env.mutex_state.init_mutex(PTHREAD_MUTEX_DEFAULT);
        env.lock_mutex(id).unwrap();
        waiting(&mut env, id);
        let mut worker = Coroutine::new(move |yielder, mut env: Environment| {
            env.with_yielder(yielder, move |env| {
                assert_eq!(env.unlock_mutex(id), Ok(0));
                assert_eq!(env.lock_mutex(id), Ok(1));
            });
            env
        });
        let CoroutineResult::Return(env) = worker.resume(env) else { panic!("disabled candidate yielded") };
        assert!(env.mutex_state.mutex_is_locked_by(id, 1));
        assert!(matches!(env.threads[0].blocked_by, ThreadBlock::Mutex(_)));
    }

    #[test]
    fn candidate_keeps_unlock_errors_and_uncontended_release() {
        let _serial = TEST_MUTEX.lock().unwrap();
        let mut env = environment();
        env.mutex_state.exit_handoff = true;
        let id = env.mutex_state.init_mutex(MutexType::PTHREAD_MUTEX_ERRORCHECK);
        let mut worker = Coroutine::new(move |yielder, mut env: Environment| {
            env.with_yielder(yielder, move |env| {
                assert_eq!(env.unlock_mutex(id), Err(EPERM));
                env.lock_mutex(id).unwrap();
                env.current_thread = 0;
                assert_eq!(env.unlock_mutex(id), Err(EPERM));
                assert!(env.mutex_state.mutex_is_locked_by(id, 1));
                env.current_thread = 1;
                assert_eq!(env.unlock_mutex(id), Ok(0));
            });
            env
        });
        assert!(matches!(worker.resume(env), CoroutineResult::Return(_)));
    }
}
