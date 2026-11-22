//! Channel-pinned bridge to a native DLL thread in this process.
use super::Event;
use crate::{
    active::{ActiveCoordinator, Binding},
    input::Owner,
    model::AppError,
};
#[path = "dll.rs"]
mod dll;
pub use dll::Capture;

#[derive(Clone)]
pub struct Gate {
    pub epoch: u64,
    pub binding: Binding,
    pub owner: Owner,
}
impl Gate {
    pub fn valid(&self, active: &ActiveCoordinator) -> bool {
        !self.owner.cancelled.is_cancelled()
            && !self
                .owner
                .command_guard
                .as_ref()
                .is_some_and(|c| c.is_cancelled())
            && active.validate(&self.binding).is_ok()
    }
    /// Never resolve a new owner or channel for a delayed native key.
    pub fn dispatch(
        &self,
        active: &ActiveCoordinator,
        input: &crate::ui_input::UiInput,
        event: Event,
    ) -> Result<bool, AppError> {
        match event {
            Event::Release { epoch } if epoch == self.epoch => {
                input.cancel(&self.owner);
                Ok(true)
            }
            Event::Key { epoch, usage, down } if epoch == self.epoch && self.valid(active) => {
                input.action(
                    self.owner.clone(),
                    self.binding.clone(),
                    crate::input::Action::Key { usage, down },
                )?;
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_events_keep_original_owner_and_drop_cancelled_or_old_epoch() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let input = crate::ui_input::UiInput::new(
            &app,
            &tokio::runtime::Handle::current(),
            Default::default(),
        );
        let gate = Gate {
            epoch: 3,
            binding: app.active.bind().unwrap(),
            owner: Owner {
                pin: Default::default(),
                id: "ui:native-test".into(),
                cancelled: Default::default(),
                command_guard: None,
            },
        };
        gate.dispatch(
            &app.active,
            &input,
            Event::Key {
                epoch: 2,
                usage: 4,
                down: true,
            },
        )
        .unwrap();
        tokio::task::yield_now().await;
        assert!(wire.calls.lock().is_empty());
        gate.dispatch(
            &app.active,
            &input,
            Event::Key {
                epoch: 3,
                usage: 4,
                down: true,
            },
        )
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while wire.calls.lock().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        gate.owner.cancelled.cancel();
        gate.dispatch(
            &app.active,
            &input,
            Event::Key {
                epoch: 3,
                usage: 5,
                down: true,
            },
        )
        .unwrap();
        assert!(gate
            .dispatch(&app.active, &input, Event::Release { epoch: 3 })
            .unwrap());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while wire.calls.lock().len() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let calls = wire.calls.lock();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, 0x101);
        assert_eq!(calls[0].2, vec![4, 0]);
        assert_eq!(calls[1].1, 0x107);
        assert_eq!(calls[0].0, calls[1].0);
        drop(calls);
        input.stop();
    }
}
