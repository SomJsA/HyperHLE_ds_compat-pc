/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `SKPaymentQueue` — StoreKit in-app purchase queue stub.

use crate::frameworks::foundation::{ns_string, NSInteger};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;

// MARK: - Per-process state

/// Singleton cache for `[SKPaymentQueue defaultQueue]`.
#[derive(Default)]
pub struct State {
    default_queue: Option<id>,
    /// Avoid injecting fake restored transactions more than once.
    fake_sent: bool,
}

impl State {
    fn get(env: &mut Environment) -> &mut State {
        &mut env.framework_state.store_kit.payment_queue
    }
}

#[derive(Default)]
struct SKPaymentQueueHostObject {
    /// SKPaymentTransactionObserver — weak reference
    observer: id,
}
impl HostObject for SKPaymentQueueHostObject {}

#[derive(Default)]
struct SKPaymentHostObject {
    product_identifier: id,
}
impl HostObject for SKPaymentHostObject {}

#[derive(Default)]
struct SKPaymentTransactionHostObject {
    transaction_state: NSInteger,
    payment: id,
    transaction_identifier: id,
    original_transaction: id,
    error: id,
}
impl HostObject for SKPaymentTransactionHostObject {}

fn fake_restore_enabled() -> bool {
    std::env::var_os("TOUCHHLE_DS_FAKE_MTX").is_some()
        || std::env::var_os("TOUCHHLE_DS_FAKE_MTX_AUTO").is_some()
}

fn make_fake_transaction(env: &mut Environment, product_identifier: &'static str) -> id {
    let identifier = ns_string::get_static_str(env, product_identifier);
    let payment: id = msg_class![env; SKPayment alloc];
    let payment: id = msg![env; payment initWithProductIdentifier:identifier];
    let transaction: id = msg_class![env; SKPaymentTransaction alloc];
    let transaction: id = msg![env; transaction init];
    {
        let host = env
            .objc
            .borrow_mut::<SKPaymentTransactionHostObject>(transaction);
        // SKPaymentTransactionStateRestored == 3
        host.transaction_state = 3;
        host.payment = payment;
        host.transaction_identifier = identifier;
        host.original_transaction = nil;
        host.error = nil;
    }
    // Keep the payment owned by the transaction; the transaction itself is
    // retained by the array passed to the observer.
    transaction
}

fn send_fake_restores(env: &mut Environment, queue: id, observer: id) {
    if observer == nil || State::get(env).fake_sent {
        return;
    }
    State::get(env).fake_sent = true;
    let transactions: id = msg_class![env; NSMutableArray new];
    for product_identifier in [
        "com.ea.mtx.827261", // 武器输出模块
        "com.ea.mtx.827281", // 护甲完整性模块
        "com.ea.mtx.827285", // 信用点激增
    ] {
        let transaction = make_fake_transaction(env, product_identifier);
        let _: () = msg![env; transactions addObject:transaction];
    }
    let sel = env.objc.register_host_selector(
        "paymentQueue:updatedTransactions:".to_string(),
        &mut env.mem,
    );
    let responds: bool = msg![env; observer respondsToSelector:sel];
    if responds {
        () = msg![env; observer paymentQueue:queue updatedTransactions:transactions];
    }
    release(env, transactions);
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation SKPaymentQueue: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKPaymentQueueHostObject {
        observer: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// MARK: - Singleton

+ (id)defaultQueue {
    // Always return the same singleton so observers are not lost between calls.
    if let Some(queue) = State::get(env).default_queue {
        return queue;
    }
    let queue: id = msg![env; this alloc];
    let queue: id = msg![env; queue init];
    // refcount=1 is our singleton retain — do NOT autorelease
    State::get(env).default_queue = Some(queue);
    log!("SKPaymentQueue defaultQueue: singleton created");
    queue
}

+ (bool)canMakePayments {
    if fake_restore_enabled() {
        return true;
    }
    // Claim payments are unavailable in the normal non-App-Store build.
    false
}

// MARK: - Init

- (id)init {
    this
}

- (())dealloc {
    let observer = env.objc.borrow::<SKPaymentQueueHostObject>(this).observer;
    release(env, observer);
    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Observers

- (())addTransactionObserver:(id)observer {
    {
        let host_obj = env.objc.borrow_mut::<SKPaymentQueueHostObject>(this);
        host_obj.observer = observer;
    }
    if std::env::var_os("TOUCHHLE_DS_FAKE_MTX_AUTO").is_some() {
        send_fake_restores(env, this, observer);
    }
}

- (())removeTransactionObserver:(id)_observer {
    let host_obj = env.objc.borrow_mut::<SKPaymentQueueHostObject>(this);
    host_obj.observer = nil;
}

// MARK: - Payment requests

- (())addPayment:(id)_payment { // SKPayment*
    log!("SKPaymentQueue addPayment: stubbed — failing transaction immediately");

    let observer = env.objc.borrow::<SKPaymentQueueHostObject>(this).observer;
    if observer == nil {
        return;
    }

    let transactions: id = msg_class![env; NSArray new];
    let sel = env.objc.register_host_selector(
        "paymentQueue:updatedTransactions:".to_string(),
        &mut env.mem,
    );
    let responds: bool = msg![env; observer respondsToSelector:sel];
    if responds {
        () = msg![env; observer paymentQueue:this updatedTransactions:transactions];
    }
}

- (())restoreCompletedTransactions {
    let observer = env.objc.borrow::<SKPaymentQueueHostObject>(this).observer;
    if observer == nil {
        return;
    }

    if fake_restore_enabled() {
        send_fake_restores(env, this, observer);
    }

    let _: () = msg![env; observer paymentQueueRestoreCompletedTransactionsFinished:this];
}

- (())restoreCompletedTransactionsWithApplicationUsername:(id)_username {
    msg![env; this restoreCompletedTransactions]
}

- (())finishTransaction:(id)_transaction { // SKPaymentTransaction*
    log!("SKPaymentQueue finishTransaction: stubbed");
}

// MARK: - Downloads (iOS 6+, always empty)

- (id)transactions {
    msg_class![env; NSArray new]
}

- (())startDownloads:(id)_downloads {
    log!("SKPaymentQueue startDownloads: stubbed");
}

- (())pauseDownloads:(id)_downloads {
    log!("SKPaymentQueue pauseDownloads: stubbed");
}

- (())resumeDownloads:(id)_downloads {
    log!("SKPaymentQueue resumeDownloads: stubbed");
}

- (())cancelDownloads:(id)_downloads {
    log!("SKPaymentQueue cancelDownloads: stubbed");
}

@end

// MARK: - SKPayment (request object)

@implementation SKPayment: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKPaymentHostObject {
        product_identifier: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)paymentWithProductIdentifier:(id)identifier { // NSString*
    let payment: id = msg_class![env; SKPayment alloc];
    let payment: id = msg![env; payment initWithProductIdentifier:identifier];
    autorelease(env, payment)
}

+ (id)paymentWithProduct:(id)product { // SKProduct*
    let identifier: id = msg![env; product productIdentifier];
    msg_class![env; SKPayment paymentWithProductIdentifier:identifier]
}

- (id)initWithProductIdentifier:(id)identifier {
    {
        let host = env.objc.borrow_mut::<SKPaymentHostObject>(this);
        host.product_identifier = identifier;
    }
    this
}

- (id)productIdentifier {
    env.objc
        .borrow::<SKPaymentHostObject>(this)
        .product_identifier
}

- (NSInteger)quantity {
    1
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

// MARK: - SKPaymentTransaction (restored transaction)

@implementation SKPaymentTransaction: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKPaymentTransactionHostObject {
        transaction_state: 0,
        payment: nil,
        transaction_identifier: nil,
        original_transaction: nil,
        error: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (NSInteger)transactionState {
    env.objc
        .borrow::<SKPaymentTransactionHostObject>(this)
        .transaction_state
}

- (id)transactionIdentifier {
    env.objc
        .borrow::<SKPaymentTransactionHostObject>(this)
        .transaction_identifier
}

- (id)payment {
    env.objc.borrow::<SKPaymentTransactionHostObject>(this).payment
}

- (id)error {
    env.objc.borrow::<SKPaymentTransactionHostObject>(this).error
}

- (id)originalTransaction {
    env.objc
        .borrow::<SKPaymentTransactionHostObject>(this)
        .original_transaction
}

- (())dealloc {
    let payment = env.objc.borrow::<SKPaymentTransactionHostObject>(this).payment;
    release(env, payment);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

