/*
 * Copyright (C) 2021 Quickwit Inc.
 *
 * Quickwit is offered under the AGPL v3.0 and as commercial software.
 * For commercial licensing, contact us at hello@quickwit.io.
 *
 * AGPL:
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */
use crate::actor_state::ActorState;
use crate::channel_with_priority::Priority;
use crate::mailbox::Command;
use crate::observation::ObservationType;
use crate::{Actor, ActorContext, ActorExitStatus, Observation};
use std::fmt;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::error;

/// An Actor Handle serves as an address to communicate with an actor.
pub struct ActorHandle<A: Actor> {
    actor_context: ActorContext<A>,
    last_state: watch::Receiver<A::ObservableState>,
    join_handle: JoinHandle<ActorExitStatus>,
}

impl<A: Actor> fmt::Debug for ActorHandle<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ActorHandle")
            .field("name", &self.actor_context.actor_instance_id())
            .finish()
    }
}

impl<A: Actor> ActorHandle<A> {
    pub(crate) fn new(
        last_state: watch::Receiver<A::ObservableState>,
        join_handle: JoinHandle<ActorExitStatus>,
        ctx: ActorContext<A>,
    ) -> Self {
        let mut interval = tokio::time::interval(crate::HEARTBEAT);
        let ctx_clone = ctx.clone();
        tokio::task::spawn(async move {
            // TODO have proper supervision.
            interval.tick().await;
            while ctx.kill_switch().is_alive() {
                interval.tick().await;
                if !ctx.progress().registered_activity_since_last_call() {
                    if ctx.get_state() == ActorState::Exit {
                        return;
                    }
                    error!(actor=%ctx.actor_instance_id(), "actor-timeout");
                    ctx.kill_switch().kill();
                    // TODO abort async tasks?
                    return;
                }
            }
        });
        ActorHandle {
            join_handle,
            last_state,
            actor_context: ctx_clone,
        }
    }

    /// Process all of the pending messages, and returns a snapshot of
    /// the observable state of the actor after this.
    ///
    /// This method is mostly useful for tests.
    ///
    /// To actually observe the state of an actor for ops purpose,
    /// prefer using the `.observe()` method.
    ///
    /// This method timeout if reaching the end of the message takes more than an HEARTBEAT.
    pub async fn process_pending_and_observe(&self) -> Observation<A::ObservableState> {
        let (tx, rx) = oneshot::channel();
        if self
            .actor_context
            .mailbox()
            .send_with_priority(Command::Observe(tx).into(), Priority::Low)
            .await
            .is_err()
        {
            error!("Failed to send message");
        }
        // The timeout is required here. If the actor fails, its inbox is properly dropped but the send channel might actually
        // prevent the onechannel Receiver from being dropped.
        let observable_state_res = tokio::time::timeout(crate::HEARTBEAT, rx).await;
        let state = self.last_observation();
        let obs_type = match observable_state_res {
            Ok(Ok(_)) => ObservationType::Alive,
            Ok(Err(_)) => ObservationType::PostMortem,
            Err(_) => ObservationType::Timeout,
        };
        Observation { obs_type, state }
    }

    /// Gracefully quit the actor, regardless of whether there are pending messages or not.
    /// Its finalize function will be called.
    pub async fn quit(&self) {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .actor_context
            .mailbox()
            .send_command(Command::Quit(tx))
            .await;
        let _ = rx.await;
    }

    /// Waits until the actor exits.
    pub async fn join(self) -> (ActorExitStatus, A::ObservableState) {
        let exit_status = self.join_handle.await.unwrap_or_else(|join_err| {
            if join_err.is_panic() {
                ActorExitStatus::Panicked
            } else {
                ActorExitStatus::Killed
            }
        });
        let observation = self.last_state.borrow().clone();
        (exit_status, observation)
    }

    /// Observe the current state.
    ///
    /// The observation will be scheduled as a command message, therefore it will be executed
    /// after the current active message and the current command queue have been processed.
    pub async fn observe(&self) -> Observation<A::ObservableState> {
        let (tx, rx) = oneshot::channel();
        if self
            .actor_context
            .mailbox()
            .send_command(Command::Observe(tx))
            .await
            .is_err()
        {
            error!("Failed to send message");
        }
        let observable_state_or_timeout = timeout(crate::HEARTBEAT, rx).await;
        let state = self.last_observation();
        let obs_type = match observable_state_or_timeout {
            Ok(Ok(())) => ObservationType::Alive,
            Ok(Err(_)) => ObservationType::PostMortem,
            Err(_) => ObservationType::Timeout,
        };
        Observation { obs_type, state }
    }

    pub fn last_observation(&self) -> A::ObservableState {
        self.last_state.borrow().clone()
    }
}
