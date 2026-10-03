use pc_keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Key {
        ch: Option<char>,
        code: KeyCode,
        pressed: bool,
    },
    MouseMove {
        dx: i16,
        dy: i16,
    },
    MouseButton {
        button: u8, // 0 = left, 1 = right, 2 = middle
        pressed: bool,
    },
    Scroll(i8),
}

const EVENT_QUEUE_CAPACITY: usize = 256;

struct EventQueue {
    buffer: [Option<InputEvent>; EVENT_QUEUE_CAPACITY],
    head: usize,
    tail: usize,
    len: usize,
}

impl EventQueue {
    const fn new() -> Self {
        Self {
            buffer: [None; EVENT_QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            len: 0,
        }
    }

    fn push(&mut self, event: InputEvent) {
        if self.len >= EVENT_QUEUE_CAPACITY {
            // Drop oldest
            self.head = (self.head + 1) % EVENT_QUEUE_CAPACITY;
            self.len -= 1;
        }
        self.buffer[self.tail] = Some(event);
        self.tail = (self.tail + 1) % EVENT_QUEUE_CAPACITY;
        self.len += 1;
    }

    fn pop(&mut self) -> Option<InputEvent> {
        if self.len == 0 {
            None
        } else {
            let item = self.buffer[self.head].take();
            self.head = (self.head + 1) % EVENT_QUEUE_CAPACITY;
            self.len -= 1;
            item
        }
    }
}

static INPUT_QUEUE: spin::Mutex<EventQueue> = spin::Mutex::new(EventQueue::new());

pub fn push_event(event: InputEvent) {
    INPUT_QUEUE.lock().push(event);
}

pub fn pop_event() -> Option<InputEvent> {
    INPUT_QUEUE.lock().pop()
}
