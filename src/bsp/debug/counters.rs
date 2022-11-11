#[derive(Default, Clone, Copy)]
pub struct Counter {
    #[cfg(feature = "task-counters")]
    cnt: u16,
}

#[cfg(feature = "task-counters")]
impl Counter {
    pub fn inc(&mut self) {
        self.cnt = self.cnt.saturating_add(1);
    }

    pub fn pop(&mut self) -> u16 {
        let val = self.cnt;
        self.cnt = 0;
        val
    }
}

#[cfg(not(feature = "task-counters"))]
impl Counter {
    pub fn inc(&mut self) {
    }

    pub fn pop(&mut self) -> u16 {
        0
    }
}
