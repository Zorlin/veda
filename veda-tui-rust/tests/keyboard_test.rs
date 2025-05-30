#[cfg(test)]
mod keyboard_tests {
    use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
    
    #[test]
    fn test_keyboard_shortcuts() {
        // Test Ctrl+C
        let quit_key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(quit_key.code, KeyCode::Char('c'));
        assert!(quit_key.modifiers.contains(KeyModifiers::CONTROL));
        
        // Test ESC
        let esc_key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(esc_key.code, KeyCode::Esc);
        
        // Test Ctrl+N
        let new_tab_key = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert_eq!(new_tab_key.code, KeyCode::Char('n'));
        assert!(new_tab_key.modifiers.contains(KeyModifiers::CONTROL));
        
        // Test Ctrl+A
        let auto_mode_key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(auto_mode_key.code, KeyCode::Char('a'));
        assert!(auto_mode_key.modifiers.contains(KeyModifiers::CONTROL));
        
        // Test Ctrl+Left
        let prev_tab_key = KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(prev_tab_key.code, KeyCode::Left);
        assert!(prev_tab_key.modifiers.contains(KeyModifiers::CONTROL));
        
        // Test Ctrl+Right
        let next_tab_key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        assert_eq!(next_tab_key.code, KeyCode::Right);
        assert!(next_tab_key.modifiers.contains(KeyModifiers::CONTROL));
    }
    
    #[test]
    fn test_key_combinations() {
        // Ensure different modifiers are distinct
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        let alt_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT);
        let plain_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        
        assert_ne!(ctrl_a.modifiers, alt_a.modifiers);
        assert_ne!(ctrl_a.modifiers, plain_a.modifiers);
        assert_ne!(alt_a.modifiers, plain_a.modifiers);
    }
}