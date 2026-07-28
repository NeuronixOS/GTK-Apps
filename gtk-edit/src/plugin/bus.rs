//! In-process message bus for cross-plugin communication.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type MessageHandler = Rc<dyn Fn(&str, &str)>;

#[derive(Clone, Default)]
pub struct MessageBus {
    inner: Rc<RefCell<HashMap<String, Vec<MessageHandler>>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self, topic: &str, handler: MessageHandler) {
        self.inner
            .borrow_mut()
            .entry(topic.to_string())
            .or_default()
            .push(handler);
    }

    pub fn publish(&self, topic: &str, payload: &str) {
        let handlers: Vec<_> = self
            .inner
            .borrow()
            .get(topic)
            .cloned()
            .unwrap_or_default();
        for h in handlers {
            h(topic, payload);
        }
    }
}
