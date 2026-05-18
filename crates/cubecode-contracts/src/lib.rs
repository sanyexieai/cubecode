//! 跨 **①～⑥** 的最小契约：事件、路由提示、会话/轮次 id。

mod events;
mod ids;
mod routes;

pub use events::{ControlEvent, ControlEventKind};
pub use ids::{SessionId, TurnContext, TurnId};
pub use routes::RouteHint;
