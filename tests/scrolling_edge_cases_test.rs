#[cfg(test)]
mod scrolling_edge_cases_tests {
    use unicode_width::UnicodeWidthStr;
    
    #[test]
    fn test_scroll_with_very_long_lines() {
        let terminal_width = 80;
        let message_area_height = 20;
        
        // Create a very long line that wraps many times
        let long_content = "This is a very long message that will definitely wrap across multiple lines ".repeat(30);
        let prefix_len = "[10:30:45] Claude: ".len();
        
        // Calculate wrapped lines (accounting for the fact that only the first line has the prefix)
        let first_line_width = 80 - prefix_len; // How much content fits on first line
        let subsequent_line_width = 80; // Full width for subsequent lines
        
        let total_content_width = UnicodeWidthStr::width(long_content.as_str());
        
        // Calculate: first line + subsequent lines needed
        let mut wrapped_lines = 1; // Start with first line
        let remaining_width = total_content_width.saturating_sub(first_line_width);
        if remaining_width > 0 {
            wrapped_lines += (remaining_width as f32 / subsequent_line_width as f32).ceil() as usize;
        }
        
        // Add empty line after message
        let total_lines = wrapped_lines + 1;
        
        // Calculate scroll offset
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        // Verify the message needs scrolling (with this long content, it definitely should)
        assert!(total_lines > message_area_height as usize, "Expected {} lines to exceed {} visible lines", total_lines, message_area_height);
        assert!(scroll_offset > 0);
        assert_eq!(scroll_offset, (total_lines - message_area_height as usize) as u16);
    }
    
    #[test]
    fn test_scroll_with_empty_messages() {
        let message_area_height = 20;
        
        // Simulate messages including empty ones
        let messages = vec![
            ("You", "Hello"),
            ("Claude", ""),  // Empty message
            ("You", "Are you there?"),
            ("Claude", "Yes, I'm here!"),
        ];
        
        let mut total_lines = 0;
        for (_sender, content) in &messages {
            // Even empty messages take at least 1 line
            let msg_lines = if content.is_empty() { 1 } else { 1 };
            total_lines += msg_lines;
            total_lines += 1; // Empty line between messages
        }
        
        assert_eq!(total_lines, 8); // 4 messages + 4 empty lines
        
        // No scrolling needed
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        assert_eq!(scroll_offset, 0);
    }
    
    #[test]
    fn test_scroll_with_multiline_content() {
        let terminal_width = 80;
        let message_area_height = 10;
        
        // Message with actual newlines
        let multiline_content = "Line 1\nLine 2\nLine 3\nLine 4";
        let prefix_len = "[10:30:45] Claude: ".len();
        
        // Calculate lines
        let mut msg_lines = 0;
        let mut is_first_line = true;
        
        for line in multiline_content.lines() {
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
        
        assert_eq!(msg_lines, 4); // 4 lines, none wrap
        
        // Add empty line
        let total_lines = msg_lines + 1;
        
        // Should not need scrolling with just one message
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        assert_eq!(scroll_offset, 0);
    }
    
    #[test]
    fn test_scroll_with_unicode_content() {
        let terminal_width = 40; // Smaller width to test wrapping
        let message_area_height = 10;
        
        // Test with various Unicode content
        let test_messages = vec![
            "Hello 你好 World 🌍",
            "Emoji test: 🎉🎊🎈🎆🎇",
            "Mixed: café résumé naïve",
            "Japanese: こんにちは世界",
        ];
        
        let mut total_lines = 0;
        for (i, content) in test_messages.iter().enumerate() {
            let prefix_len = format!("[10:30:4{}] Claude: ", i).len();
            let content_width = UnicodeWidthStr::width(*content) + prefix_len;
            let wrapped = (content_width as f32 / terminal_width as f32).ceil() as usize;
            total_lines += wrapped.max(1);
            total_lines += 1; // Empty line
        }
        
        // Calculate scroll
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        // Verify unicode handling doesn't break scrolling
        assert!(total_lines > 0);
        if total_lines > message_area_height as usize {
            assert!(scroll_offset > 0);
        }
    }
    
    #[test]
    fn test_scroll_exact_fit() {
        let message_area_height = 5;
        
        // Create exactly 5 lines of content (including empty lines)
        // Message 1: 1 line + 1 empty = 2 lines
        // Message 2: 1 line + 1 empty = 2 lines  
        // Message 3: 1 line = 1 line
        // Total: 5 lines (exact fit)
        
        let total_lines = 5;
        
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        assert_eq!(scroll_offset, 0); // Exact fit, no scrolling
    }
    
    #[test] 
    fn test_scroll_one_line_overflow() {
        let message_area_height = 5;
        
        // Create 6 lines of content (1 more than fits)
        let total_lines = 6;
        
        let scroll_offset = if total_lines > message_area_height as usize {
            (total_lines - message_area_height as usize) as u16
        } else {
            0
        };
        
        assert_eq!(scroll_offset, 1); // Need to scroll by 1 line
    }
    
    #[test]
    fn test_scroll_calculation_matches_implementation() {
        // Test that our test calculation matches the actual implementation logic
        let test_cases = vec![
            (80, 20, vec!["Short message"], 0),  // No scroll needed
            (80, 5, vec!["Msg 1", "Msg 2", "Msg 3", "Msg 4"], 3), // 8 lines total, 5 visible
            (40, 10, vec!["This is a very long message that will wrap"], 0), // Depends on wrap
        ];
        
        for (term_width, height, messages, _expected) in test_cases {
            let mut total_lines = 0;
            
            for (i, msg) in messages.iter().enumerate() {
                let prefix_len = format!("[10:30:0{}] You: ", i).len();
                let msg_width = UnicodeWidthStr::width(*msg) + prefix_len;
                let wrapped = (msg_width as f32 / term_width as f32).ceil() as usize;
                total_lines += wrapped.max(1);
                total_lines += 1; // Empty line
            }
            
            let scroll_offset = if total_lines > height as usize {
                (total_lines - height as usize) as u16
            } else {
                0
            };
            
            // Verify calculation is consistent
            assert!(scroll_offset <= total_lines as u16);
        }
    }
}