use chrono::{DateTime, Local};
use std::time::Instant;
use uuid::Uuid;

/// Configuration for stall detection behavior
#[derive(Debug, Clone)]
pub struct StallConfig {
    /// Initial delay before detecting a stall (seconds)
    pub initial_delay_seconds: i64,
    /// Maximum delay before detecting a stall (seconds)
    pub max_delay_seconds: i64,
    /// Minimum time between stall checks (seconds)
    pub min_check_interval: i64,
}

impl Default for StallConfig {
    fn default() -> Self {
        Self {
            initial_delay_seconds: 10,
            max_delay_seconds: 30,
            min_check_interval: 5,
        }
    }
}

/// Tracks stall detection state for a conversation
#[derive(Debug, Clone)]
pub struct StallTracker {
    /// Last time there was activity in the conversation
    pub last_activity: DateTime<Local>,
    /// Current delay before triggering stall detection
    pub current_delay_seconds: i64,
    /// Whether a stall check has been sent and we're waiting for response
    pub stall_check_sent: bool,
    /// Whether stall intervention is currently in progress
    pub intervention_in_progress: bool,
    /// Whether the conversation is actively processing something
    pub is_processing: bool,
    /// Configuration for stall behavior
    pub config: StallConfig,
    /// Last time we checked for stalls (to prevent spam)
    pub last_stall_check: Option<DateTime<Local>>,
}

impl StallTracker {
    pub fn new() -> Self {
        Self {
            last_activity: Local::now(),
            current_delay_seconds: StallConfig::default().initial_delay_seconds,
            stall_check_sent: false,
            intervention_in_progress: false,
            is_processing: false,
            config: StallConfig::default(),
            last_stall_check: None,
        }
    }

    /// Update activity timestamp and reset stall detection state
    pub fn on_activity(&mut self) {
        self.last_activity = Local::now();
        self.stall_check_sent = false;
        // Increase delay when there's activity to be less aggressive
        self.current_delay_seconds = (self.current_delay_seconds * 2).min(self.config.max_delay_seconds);
        tracing::debug!("Activity detected, stall delay increased to {} seconds", self.current_delay_seconds);
    }

    /// Mark that processing has started
    pub fn start_processing(&mut self) {
        self.is_processing = true;
        self.last_activity = Local::now();
    }

    /// Mark that processing has finished
    pub fn stop_processing(&mut self) {
        self.is_processing = false;
        self.last_activity = Local::now();
    }

    /// Mark that a stall check has been initiated
    pub fn mark_stall_check_sent(&mut self) {
        self.stall_check_sent = true;
        self.intervention_in_progress = true;
        self.last_stall_check = Some(Local::now());
    }

    /// Mark that stall intervention has completed
    pub fn mark_intervention_complete(&mut self) {
        self.intervention_in_progress = false;
        self.stall_check_sent = false;
        // Reset delay to initial value after intervention
        self.current_delay_seconds = self.config.initial_delay_seconds;
    }

    /// Check if we should trigger stall detection
    pub fn should_check_for_stall(&self, has_user_messages: bool) -> bool {
        // Don't trigger if already processing, check sent, or intervention in progress
        if self.is_processing || self.stall_check_sent || self.intervention_in_progress {
            tracing::debug!("Stall check blocked: processing={}, check_sent={}, intervention={}", 
                self.is_processing, self.stall_check_sent, self.intervention_in_progress);
            return false;
        }

        // Don't trigger if user hasn't sent any messages yet
        if !has_user_messages {
            tracing::debug!("Stall check blocked: no user messages yet");
            return false;
        }

        // Check if enough time has passed since last stall check
        if let Some(last_check) = self.last_stall_check {
            let time_since_last_check = Local::now().signed_duration_since(last_check).num_seconds();
            if time_since_last_check < self.config.min_check_interval {
                tracing::debug!("Stall check blocked: only {} seconds since last check (min: {})", 
                    time_since_last_check, self.config.min_check_interval);
                return false;
            }
        }

        let elapsed = Local::now().signed_duration_since(self.last_activity).num_seconds();
        let should_stall = elapsed > self.current_delay_seconds;
        
        if should_stall {
            tracing::info!("Stall condition met: {} seconds elapsed (threshold: {})", 
                elapsed, self.current_delay_seconds);
        } else {
            tracing::debug!("No stall: {} seconds elapsed (threshold: {})", 
                elapsed, self.current_delay_seconds);
        }

        should_stall
    }

    /// Get the current elapsed time since last activity
    pub fn get_elapsed_seconds(&self) -> i64 {
        Local::now().signed_duration_since(self.last_activity).num_seconds()
    }
}

/// Information about a detected stall
#[derive(Debug, Clone)]
pub struct StallInfo {
    pub instance_id: Uuid,
    pub delay_seconds: i64,
    pub claude_message: String,
    pub user_context: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_initial_state() {
        let tracker = StallTracker::new();
        assert!(!tracker.should_check_for_stall(true));
        assert!(!tracker.is_processing);
        assert!(!tracker.stall_check_sent);
        assert!(!tracker.intervention_in_progress);
    }

    #[test]
    fn test_activity_resets_stall() {
        let mut tracker = StallTracker::new();
        // Simulate time passing
        tracker.last_activity = Local::now() - Duration::seconds(15);
        
        // Should trigger stall
        assert!(tracker.should_check_for_stall(true));
        
        // Activity resets it
        tracker.on_activity();
        assert!(!tracker.should_check_for_stall(true));
    }

    #[test]
    fn test_no_stall_without_user_messages() {
        let mut tracker = StallTracker::new();
        tracker.last_activity = Local::now() - Duration::seconds(15);
        
        // Should not trigger without user messages
        assert!(!tracker.should_check_for_stall(false));
        
        // Should trigger with user messages
        assert!(tracker.should_check_for_stall(true));
    }

    #[test]
    fn test_processing_blocks_stall() {
        let mut tracker = StallTracker::new();
        tracker.last_activity = Local::now() - Duration::seconds(15);
        tracker.start_processing();
        
        // Should not trigger while processing
        assert!(!tracker.should_check_for_stall(true));
        
        tracker.stop_processing();
        // Should trigger after processing stops (if enough time passed)
        assert!(tracker.should_check_for_stall(true));
    }

    #[test]
    fn test_min_check_interval() {
        let mut tracker = StallTracker::new();
        tracker.config.min_check_interval = 5;
        tracker.last_activity = Local::now() - Duration::seconds(15);
        
        // First check should work
        assert!(tracker.should_check_for_stall(true));
        
        // Mark stall check sent
        tracker.mark_stall_check_sent();
        tracker.mark_intervention_complete();
        
        // Should not trigger again immediately
        assert!(!tracker.should_check_for_stall(true));
        
        // Simulate time passing beyond min interval
        tracker.last_stall_check = Some(Local::now() - Duration::seconds(6));
        tracker.last_activity = Local::now() - Duration::seconds(15);
        
        // Should trigger again now
        assert!(tracker.should_check_for_stall(true));
    }
}