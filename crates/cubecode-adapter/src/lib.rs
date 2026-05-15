//! ① **Adapter**：占位实现——将「一行用户输入」写入 inbox；真实终端/网络后续替换。

use cubecode_contracts::ControlEvent;
use cubecode_inbox::Inbox;

pub fn enqueue_user_line(inbox: &mut Inbox, line: impl Into<String>) {
    inbox.push(ControlEvent::UserLine(line.into()));
}

pub fn enqueue_shutdown(inbox: &mut Inbox) {
    inbox.push(ControlEvent::Shutdown);
}
