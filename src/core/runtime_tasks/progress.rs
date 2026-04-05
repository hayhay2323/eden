use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub current_step: usize,
    pub total_steps: usize,
    pub message: Option<String>,
}

impl TaskProgress {
    pub fn new(total_steps: usize) -> Self {
        Self {
            current_step: 0,
            total_steps,
            message: None,
        }
    }

    pub fn advance(&mut self, message: impl Into<String>) {
        self.current_step = (self.current_step + 1).min(self.total_steps);
        self.message = Some(message.into());
    }

    pub fn percentage(&self) -> f64 {
        if self.total_steps == 0 {
            return 0.0;
        }
        (self.current_step as f64 / self.total_steps as f64) * 100.0
    }

    pub fn is_complete(&self) -> bool {
        self.current_step >= self.total_steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_percentage() {
        let mut p = TaskProgress::new(10);
        for i in 0..5 {
            p.advance(format!("step {}", i));
        }
        assert!(
            (p.percentage() - 50.0).abs() < 1e-9,
            "Expected 50%, got {}%",
            p.percentage()
        );
    }

    #[test]
    fn progress_complete() {
        let mut p = TaskProgress::new(3);
        for i in 0..3 {
            p.advance(format!("step {}", i));
        }
        assert!(p.is_complete());
    }
}
