//! Windowing and watermarking for event-time processing.

mod session;
mod sliding;
mod tumbling;
mod watermark;
mod window;

pub use session::{SessionAssigner, SessionWindow, SessionWindowConfig};
pub use sliding::{SlidingAssigner, SlidingWindow, SlidingWindowConfig};
pub use tumbling::{TumblingAssigner, TumblingWindow, TumblingWindowConfig};
pub use watermark::{
    MultiSourceWatermarkManager, PeriodicWatermarkGenerator, PunctuatedWatermarkGenerator,
    Watermark, WatermarkConfig, WatermarkGenerator, WatermarkStrategy,
};
pub use window::{
    CountTrigger, EventTimeSessionWindows, EventTimeTrigger, ProcessingTimeSessionWindows,
    TriggerResult, Window, WindowAssigner, WindowState, WindowTrigger,
};
