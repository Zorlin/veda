#[cfg(test)]
mod scrolling_tests {
    use unicode_width::UnicodeWidthStr;
    
    #[test]
    fn test_scroll_calculation() {
        // Test the scroll offset calculation logic
        let terminal_width = 80;
        let message_area_height = 20;
        
        // Simulate messages with their display properties
        struct TestMessage {
            timestamp: &'static str,
            sender: &'static str,
            content: &'static str,
        }
        
        let messages = vec![
            TestMessage { timestamp: "[10:30:45]", sender: "You", content: "Hello" },
            TestMessage { timestamp: "[10:30:46]", sender: "Claude", content: "Hi there! How can I help you today?" },
            TestMessage { timestamp: "[10:30:47]", sender: "You", content: "Can you write a very long message that will wrap across multiple lines in the terminal?" },
            TestMessage { timestamp: "[10:30:48]", sender: "Claude", content: "Sure! Here's a very long message that will definitely wrap across multiple lines in a standard 80-character terminal width. This message contains enough text to demonstrate the wrapping behavior and ensure that our scroll calculation properly accounts for wrapped lines." },
        ];
        
        // Calculate total lines needed
        let mut total_lines = 0;
        
        for msg in &messages {
            let prefix_len = msg.timestamp.len() + msg.sender.len() + 3; // ": " and space
            
            let mut msg_lines = 0;
            let mut is_first_line = true;
            
            for line in msg.content.lines() {
                if line.is_empty() {
                    msg_lines += 1;
                } else {
                    let line_width = if is_first_line {
                        UnicodeWidthStr::width(line) + prefix_len
                    } else {
                        UnicodeWidthStr::width(line)
                    };
                    let wrapped = (line_width as f32 / terminal_width as f32).ceil() as usize;
                    msg_lines += wrapped.max(1);
                    is_first_line = false;
                }
            }
            
            if msg_lines == 0 {
                msg_lines = 1;
            }
            
            total_lines += msg_lines;
            total_lines += 1; // Empty line between messages
        }
        
        // Calculate scroll offset
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        // Verify the calculation
        assert!(total_lines > 0, "Should have calculated some lines");
        assert!(scroll_offset < total_lines as u16, "Scroll offset should be less than total lines");
        
        // Test with very long content
        let long_content = "a".repeat(1000);
        let long_line_width = UnicodeWidthStr::width(long_content.as_str());
        let wrapped_lines = (long_line_width as f32 / terminal_width as f32).ceil() as usize;
        assert!(wrapped_lines > 10, "Long content should wrap to many lines");
    }
    
    #[test]
    fn test_unicode_width_handling() {
        // Test that unicode characters are handled correctly
        let test_cases = vec![
            ("Hello", 5),
            ("你好", 4), // Chinese characters are typically 2 width each
            ("🎉", 2),   // Emoji are typically 2 width
            ("café", 4), // Accented characters
        ];
        
        for (text, expected_width) in test_cases {
            let width = UnicodeWidthStr::width(text);
            assert_eq!(width, expected_width, "Width of '{}' should be {}", text, expected_width);
        }
    }
    
    #[test]
    fn test_scroll_offset_boundaries() {
        // Test edge cases for scroll offset calculation
        let test_cases = vec![
            (10, 20, 0),  // Content fits in view, no scroll
            (25, 20, 5),  // Content exceeds view by 5 lines
            (100, 20, 80), // Large content
            (20, 20, 0),  // Content exactly fits
            (21, 20, 1),  // Content exceeds by 1 line
        ];
        
        for (total_lines, visible_lines, expected_offset) in test_cases {
            let offset = if total_lines > visible_lines {
                total_lines - visible_lines
            } else {
                0
            };
            assert_eq!(offset, expected_offset, 
                "For {} total lines and {} visible lines, offset should be {}", 
                total_lines, visible_lines, expected_offset);
        }
    }
}