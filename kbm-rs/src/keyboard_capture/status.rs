//! Capture lifecycle metadata only; never key identities or typed content.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[repr(u32)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Dormant = 0,
    Requested,
    WaitingForeground,
    Armed,
    Disabled,
    OwnerCancelled,
    StreamEnded,
    StaleChannel,
    InstallFailed,
    QueueFull,
    ThreadFailed,
    Stopped,
}
impl Phase {
    pub fn code(self) -> &'static str {
        match self {
            Self::Dormant => "HOOK_DORMANT",
            Self::Requested => "HOOK_REQUESTED",
            Self::WaitingForeground => "HOOK_WAITING_FOREGROUND",
            Self::Armed => "HOOK_ARMED",
            Self::Disabled => "HOOK_DISABLED",
            Self::OwnerCancelled => "HOOK_OWNER_CANCELLED",
            Self::StreamEnded => "HOOK_STREAM_ENDED",
            Self::StaleChannel => "HOOK_STALE_CHANNEL",
            Self::InstallFailed => "HOOK_INSTALL_FAILED",
            Self::QueueFull => "HOOK_QUEUE_FULL",
            Self::ThreadFailed => "HOOK_THREAD_FAILED",
            Self::Stopped => "HOOK_STOPPED",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Dormant => "후크 대기",
            Self::Requested => "후크 설치 요청 중",
            Self::WaitingForeground => "후크 대기 · 앱 포커스 확인 중",
            Self::Armed => "특수 키 후크 활성",
            Self::Disabled => "후크 비활성",
            Self::OwnerCancelled => "후크 해제 · 입력 소유권 종료",
            Self::StreamEnded => "후크 해제 · 영상 종료",
            Self::StaleChannel => "후크 해제 · 채널 변경",
            Self::InstallFailed => "후크 설치 실패",
            Self::QueueFull => "후크 입력 큐 초과",
            Self::ThreadFailed => "네이티브 키보드 오류",
            Self::Stopped => "네이티브 키보드 종료",
        }
    }
}
#[derive(Clone, Debug, Default, serde::Serialize, schemars::JsonSchema)]
pub struct Status {
    pub helper_pid: Option<u32>,
    pub native_thread_id: Option<u32>,
    pub received_events: u64,
    pub queued_events: u64,
    pub rejected_events: u64,
    pub phase: Phase,
    pub requested_epoch: u64,
    pub armed_epoch: u64,
    pub installed: bool,
    pub foreground: bool,
    pub install_count: u64,
    pub callbacks: u64,
    pub reentrant_callbacks: u64,
    pub swallowed: u64,
    pub window_callbacks: u64,
    pub window_swallowed: u64,
    pub win32_error: Option<u32>,
}
impl Status {
    pub fn active(&self) -> bool {
        self.installed
            && self.armed_epoch != 0
            && self.armed_epoch == self.requested_epoch
            && self.phase == Phase::Armed
    }
}
