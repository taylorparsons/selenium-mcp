use crate::flow::FlowStep;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordedActions {
    steps: Vec<FlowStep>,
    unsaveable_secret: bool,
}

#[derive(Clone, Default)]
pub struct ActionLog(Arc<Mutex<RecordedActions>>);

impl ActionLog {
    pub fn append(&self, step: FlowStep) {
        let mut recorded = self.0.lock().unwrap();
        if let FlowStep::TypeText {
            text, secret: true, ..
        } = &step
        {
            if crate::flow::secret_env_name(text).is_none() {
                recorded.unsaveable_secret = true;
                return;
            }
        }
        recorded.steps.push(step);
    }

    /// Drains and returns every logged step — flow_save calls this once,
    /// after which the log is empty again so the next flow_save starts clean.
    pub fn take_all(&self) -> Result<Vec<FlowStep>, crate::error::AppError> {
        let mut recorded = self.0.lock().unwrap();
        if recorded.unsaveable_secret {
            return Err(crate::error::AppError::InvalidTarget(
                "cannot save this session: sensitive text was typed without env_var; start a new session and use env_var for sensitive input".into(),
            ));
        }
        Ok(std::mem::take(&mut recorded.steps))
    }

    pub fn clear(&self) {
        *self.0.lock().unwrap() = RecordedActions::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_take_all_returns_steps_in_order_and_clears() {
        let log = ActionLog::default();
        log.append(FlowStep::Navigate {
            url: "http://example.com".to_string(),
        });
        log.append(FlowStep::AssertThat {
            that: crate::flow::AssertCondition {
                text_visible: Some("hi".to_string()),
                ..Default::default()
            },
        });

        let steps = log.take_all().unwrap();

        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0], FlowStep::Navigate { .. }));
        assert!(
            log.take_all().unwrap().is_empty(),
            "take_all should drain the log"
        );
    }

    #[test]
    fn raw_secret_never_enters_the_recorded_actions() {
        let log = ActionLog::default();
        log.append(FlowStep::TypeText {
            target: crate::flow::Target {
                css: Some("#password".into()),
                xpath: None,
                semantic: None,
            },
            text: "audit-sensitive-value".into(),
            clear_first: false,
            secret: true,
        });
        let actions = log.take_all();
        assert!(
            actions.is_err(),
            "a session with an unrecordable secret must fail closed"
        );
        let serialized = format!("{actions:?}");
        assert!(!serialized.contains("audit-sensitive-value"));
    }
}
