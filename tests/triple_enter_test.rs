#[cfg(test)]
mod triple_enter_tests {
    use std::time::{Duration, Instant};
    
    #[test]
    fn test_triple_enter_detection() {
        // Simulate triple enter press tracking
        let mut enter_press_count = 0;
        let mut last_enter_time: Option<Instant> = None;
        
        // Helper function to simulate enter press logic
        fn handle_enter_press(
            now: Instant,
            last_time: &mut Option<Instant>,
            count: &mut usize,
        ) -> bool {
            let should_interrupt = if let Some(last) = last_time {
                if now.duration_since(*last).as_millis() < 500 {
                    *count += 1;
                    *count >= 3
                } else {
                    *count = 1;
                    false
                }
            } else {
                *count = 1;
                false
            };
            *last_time = Some(now);
            should_interrupt
        }
        
        // Test case 1: Three quick enters should trigger interrupt
        let t0 = Instant::now();
        assert_eq!(handle_enter_press(t0, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 1);
        
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(handle_enter_press(t1, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 2);
        
        let t2 = t1 + Duration::from_millis(100);
        assert_eq!(handle_enter_press(t2, &mut last_enter_time, &mut enter_press_count), true);
        assert_eq!(enter_press_count, 3);
        
        // Test case 2: Reset count after interrupt
        enter_press_count = 0;
        let t3 = t2 + Duration::from_millis(100);
        assert_eq!(handle_enter_press(t3, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 1);
        
        // Test case 3: Slow enters should not trigger interrupt
        enter_press_count = 0;
        last_enter_time = None;
        
        let t4 = Instant::now();
        assert_eq!(handle_enter_press(t4, &mut last_enter_time, &mut enter_press_count), false);
        
        let t5 = t4 + Duration::from_millis(600); // Too slow
        assert_eq!(handle_enter_press(t5, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 1); // Reset to 1, not 2
        
        let t6 = t5 + Duration::from_millis(100);
        assert_eq!(handle_enter_press(t6, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 2);
        
        let t7 = t6 + Duration::from_millis(100);
        assert_eq!(handle_enter_press(t7, &mut last_enter_time, &mut enter_press_count), true);
        assert_eq!(enter_press_count, 3);
    }
    
    #[test]
    fn test_triple_enter_timing_edge_cases() {
        let mut enter_press_count = 0;
        let mut last_enter_time: Option<Instant> = None;
        
        // Helper function (same as above)
        fn handle_enter_press(
            now: Instant,
            last_time: &mut Option<Instant>,
            count: &mut usize,
        ) -> bool {
            let should_interrupt = if let Some(last) = last_time {
                if now.duration_since(*last).as_millis() < 500 {
                    *count += 1;
                    *count >= 3
                } else {
                    *count = 1;
                    false
                }
            } else {
                *count = 1;
                false
            };
            *last_time = Some(now);
            should_interrupt
        }
        
        // Edge case: Exactly at 500ms boundary
        let t0 = Instant::now();
        handle_enter_press(t0, &mut last_enter_time, &mut enter_press_count);
        
        let t1 = t0 + Duration::from_millis(499); // Just under threshold
        assert_eq!(handle_enter_press(t1, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 2);
        
        let t2 = t1 + Duration::from_millis(499); // Still under threshold
        assert_eq!(handle_enter_press(t2, &mut last_enter_time, &mut enter_press_count), true);
        assert_eq!(enter_press_count, 3);
        
        // Reset and test just over threshold
        enter_press_count = 0;
        last_enter_time = None;
        
        let t3 = Instant::now();
        handle_enter_press(t3, &mut last_enter_time, &mut enter_press_count);
        
        let t4 = t3 + Duration::from_millis(501); // Just over threshold
        assert_eq!(handle_enter_press(t4, &mut last_enter_time, &mut enter_press_count), false);
        assert_eq!(enter_press_count, 1); // Reset due to timeout
    }
}