//! Track guest touches actually delivered, so cancellation releases exactly once.
use std::collections::HashMap;
use std::hash::Hash;

pub struct HeldTouches<K: Eq + Hash>(HashMap<K, (f32, f32)>);
impl<K: Eq + Hash + Copy> HeldTouches<K> {
    pub fn new() -> Self { Self(HashMap::new()) }
    pub fn down(&mut self, touches: &HashMap<K, (f32, f32)>) { self.0.extend(touches.iter().map(|(k,v)| (*k,*v))); }
    pub fn moved(&mut self, touches: &HashMap<K, (f32, f32)>) {
        for (id, coords) in touches { if let Some(old) = self.0.get_mut(id) { *old = *coords; } }
    }
    pub fn up(&mut self, touches: &HashMap<K, (f32, f32)>) { for id in touches.keys() { self.0.remove(id); } }
    #[cfg(test)]
    pub fn release_all(&mut self) -> HashMap<K, (f32, f32)> { std::mem::take(&mut self.0) }
    pub fn snapshot(&self) -> HashMap<K, (f32, f32)> { self.0.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_uses_last_delivered_coords_and_is_idempotent() {
        let mut held = HeldTouches::new();
        held.down(&HashMap::from([(1, (4.0,5.0)), (2, (8.0,9.0))]));
        held.moved(&HashMap::from([(1, (6.0,7.0)), (99, (0.0,0.0))]));
        held.up(&HashMap::from([(2, (8.0,9.0))]));
        assert_eq!(held.release_all(), HashMap::from([(1, (6.0,7.0))]));
        assert!(held.release_all().is_empty());
    }
    #[test]
    fn repeated_cancellation_keeps_release_until_it_is_delivered() {
        let mut held = HeldTouches::new();
        held.down(&HashMap::from([(1, (4.0,5.0))]));
        let release = held.snapshot();
        assert_eq!(held.snapshot(), release);
        held.up(&release);
        assert!(held.snapshot().is_empty());
    }
}
