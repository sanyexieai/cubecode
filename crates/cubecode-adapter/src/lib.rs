//! ① **适配层**：将用户输入转为语义事件写入收件箱。

use cubecode_contracts::ControlEvent;
use cubecode_inbox::Inbox;

pub fn enqueue_user_line(inbox: &mut Inbox, line: impl Into<String>) {
    let text = line.into();
    let bytes = text.len();
    tracing::info!(
        target: "cubecode.adapter",
        bytes,
        "①适配层：入队用户消息"
    );
    inbox.push(ControlEvent::UserLine(text));
}

pub fn enqueue_shutdown(inbox: &mut Inbox) {
    tracing::info!(target: "cubecode.adapter", "①适配层：入队关闭事件");
    inbox.push(ControlEvent::Shutdown);
}
