use proc_macro2::{TokenStream, Ident, Span};
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

use crate::{impl_enum_to_tokens, impl_enum_tuple_to_tokens};

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
#[schemars(rename = "CustomAction")]
pub enum Action {
    /// Modify LED lightning
    Led(LedAction),
    /// Use mouse emulation
    Mouse(MouseAction),
    /// Send USB HID consumer page keys
    Consumer(ConsumerKey),
    /// Perform special firmware-related actions
    Firmware(FirmwareAction)
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum LedAction {
    /// Cycle through available LED configurations
    Cycle(Inc),
    /// Modify global brightness
    Brightness(Inc),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum MouseAction {
    /// Key emulates a mouse key
    Click(MouseButton),
    /// Key performs mouse movement when held
    Move(MouseMovement),
    /// Key changes mouse sensitivity
    Sensitivity(Inc),
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum MouseButton {
    Left,
    Mid,
    Right,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum MouseMovement {
    Up,
    Down,
    Left,
    Right,
    WheelUp,
    WheelDown,
    PanLeft,
    PanRight,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum FirmwareAction {
    AllowBootloader,
    JumpToBootloader,
    Reboot,
    InfiniteLoop,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum ConsumerKey {
    Unassigned,
    ConsumerControl,
    NumericKeyPad,
    ProgrammableButtons,
    Microphone,
    Headphone,
    GraphicEqualizer,
    Plus10,
    Plus100,
    AmPm,
    Power,
    Reset,
    Sleep,
    SleepAfter,
    SleepMode,
    Illumination,
    FunctionButtons,
    Menu,
    MenuPick,
    MenuUp,
    MenuDown,
    MenuLeft,
    MenuRight,
    MenuEscape,
    MenuValueIncrease,
    MenuValueDecrease,
    DataOnScreen,
    ClosedCaption,
    ClosedCaptionSelect,
    VcrTv,
    BroadcastMode,
    Snapshot,
    Still,
    Selection,
    AssignSelection,
    ModeStep,
    RecallLast,
    EnterChannel,
    OrderMovie,
    Channel,
    MediaSelection,
    MediaSelectComputer,
    MediaSelectTV,
    MediaSelectWWW,
    MediaSelectDVD,
    MediaSelectTelephone,
    MediaSelectProgramGuide,
    MediaSelectVideoPhone,
    MediaSelectGames,
    MediaSelectMessages,
    MediaSelectCD,
    MediaSelectVCR,
    MediaSelectTuner,
    Quit,
    Help,
    MediaSelectTape,
    MediaSelectCable,
    MediaSelectSatellite,
    MediaSelectSecurity,
    MediaSelectHome,
    MediaSelectCall,
    ChannelIncrement,
    ChannelDecrement,
    MediaSelectSAP,
    VCRPlus,
    Once,
    Daily,
    Weekly,
    Monthly,
    Play,
    Pause,
    Record,
    FastForward,
    Rewind,
    ScanNextTrack,
    ScanPreviousTrack,
    Stop,
    Eject,
    RandomPlay,
    SelectDisc,
    EnterDisc,
    Repeat,
    Tracking,
    TrackNormal,
    SlowTracking,
    FrameForward,
    FrameBack,
    Mark,
    ClearMark,
    RepeatFromMark,
    ReturnToMark,
    SearchMarkForward,
    SearchMarkBackwards,
    CounterReset,
    ShowCounter,
    TrackingIncrement,
    TrackingDecrement,
    StopEject,
    PlayPause,
    PlaySkip,
    Volume,
    Balance,
    Mute,
    Bass,
    Treble,
    BassBoost,
    SurroundMode,
    Loudness,
    MPX,
    VolumeIncrement,
    VolumeDecrement,
    SpeedSelect,
    PlaybackSpeed,
    StandardPlay,
    LongPlay,
    ExtendedPlay,
    Slow,
    FanEnable,
    FanSpeed,
    LightEnable,
    LightIlluminationLevel,
    ClimateControlEnable,
    RoomTemperature,
    SecurityEnable,
    FireAlarm,
    PoliceAlarm,
    Proximity,
    Motion,
    DuressAlarm,
    HoldupAlarm,
    MedicalAlarm,
    BalanceRight,
    BalanceLeft,
    BassIncrement,
    BassDecrement,
    TrebleIncrement,
    TrebleDecrement,
    SpeakerSystem,
    ChannelLeft,
    ChannelRight,
    ChannelCenter,
    ChannelFront,
    ChannelCenterFront,
    ChannelSide,
    ChannelSurround,
    ChannelLowFrequencyEnhancement,
    ChannelTop,
    ChannelUnknown,
    SubChannel,
    SubChannelIncrement,
    SubChannelDecrement,
    AlternateAudioIncrement,
    AlternateAudioDecrement,
    ApplicationLaunchButtons,
    ALLaunchButtonConfigurationTool,
    ALProgrammableButtonConfiguration,
    ALConsumerControlConfiguration,
    ALWordProcessor,
    ALTextEditor,
    ALSpreadsheet,
    ALGraphicsEditor,
    ALPresentationApp,
    ALDatabaseApp,
    ALEmailReader,
    ALNewsreader,
    ALVoicemail,
    ALContactsAddressBook,
    ALCalendarSchedule,
    ALTaskProjectManager,
    ALLogJournalTimecard,
    ALCheckbookFinance,
    ALCalculator,
    ALAvCapturePlayback,
    ALLocalMachineBrowser,
    ALLanWanBrowser,
    ALInternetBrowser,
    ALRemoteNetworkingISPConnect,
    ALNetworkConference,
    ALNetworkChat,
    ALTelephonyDialer,
    ALLogon,
    ALLogoff,
    ALLogonLogoff,
    ALTerminalLockScreensaver,
    ALControlPanel,
    ALCommandLineProcessorRun,
    ALProcessTaskManager,
    ALSelectTaskApplication,
    ALNextTaskApplication,
    ALPreviousTaskApplication,
    ALPreemptiveHaltTaskApplication,
    ALIntegratedHelpCenter,
    ALDocuments,
    ALThesaurus,
    ALDictionary,
    ALDesktop,
    ALSpellCheck,
    ALGrammarCheck,
    ALWirelessStatus,
    ALKeyboardLayout,
    ALVirusProtection,
    ALEncryption,
    ALScreenSaver,
    ALAlarms,
    ALClock,
    ALFileBrowser,
    ALPowerStatus,
    ALImageBrowser,
    ALAudioBrowser,
    ALMovieBrowser,
    ALDigitalRightsManager,
    ALDigitalWallet,
    ALInstantMessaging,
    ALOemFeaturesTipsTutorialBrowser,
    ALOemHelp,
    ALOnlineCommunity,
    ALEntertainmentContentBrowser,
    ALOnlineShoppingBrowser,
    ALSmartCardInformationHelp,
    ALMarketMonitorFinanceBrowser,
    ALCustomizedCorporateNewsBrowser,
    ALOnlineActivityBrowser,
    ALResearchSearchBrowser,
    ALAudioPlayer,
    GenericGUIApplicationControls,
    ACNew,
    ACOpen,
    ACClose,
    ACExit,
    ACMaximize,
    ACMinimize,
    ACSave,
    ACPrint,
    ACProperties,
    ACUndo,
    ACCopy,
    ACCut,
    ACPaste,
    ACSelectAll,
    ACFind,
    ACFindAndReplace,
    ACSearch,
    ACGoTo,
    ACHome,
    ACBack,
    ACForward,
    ACStop,
    ACRefresh,
    ACPreviousLink,
    ACNextLink,
    ACBookmarks,
    ACHistory,
    ACSubscriptions,
    ACZoomIn,
    ACZoomOut,
    ACZoom,
    ACFullScreenView,
    ACNormalView,
    ACViewToggle,
    ACScrollUp,
    ACScrollDown,
    ACScroll,
    ACPanLeft,
    ACPanRight,
    ACPan,
    ACNewWindow,
    ACTileHorizontally,
    ACTileVertically,
    ACFormat,
    ACEdit,
    ACBold,
    ACItalics,
    ACUnderline,
    ACStrikethrough,
    ACSubscript,
    ACSuperscript,
    ACAllCaps,
    ACRotate,
    ACResize,
    ACFlipHorizontal,
    ACFlipVertical,
    ACMirrorHorizontal,
    ACMirrorVertical,
    ACFontSelect,
    ACFontColor,
    ACFontSize,
    ACJustifyLeft,
    ACJustifyCenterH,
    ACJustifyRight,
    ACJustifyBlockH,
    ACJustifyTop,
    ACJustifyCenterV,
    ACJustifyBottom,
    ACJustifyBlockV,
    ACIndentDecrease,
    ACIndentIncrease,
    ACNumberedList,
    ACRestartNumbering,
    ACBulletedList,
    ACPromote,
    ACDemote,
    ACYes,
    ACNo,
    ACCancel,
    ACCatalog,
    ACBuyCheckout,
    ACAddToCart,
    ACExpand,
    ACExpandAll,
    ACCollapse,
    ACCollapseAll,
    ACPrintPreview,
    ACPasteSpecial,
    ACInsertMode,
    ACDelete,
    ACLock,
    ACUnlock,
    ACProtect,
    ACUnprotect,
    ACAttachComment,
    ACDeleteComment,
    ACViewComment,
    ACSelectWord,
    ACSelectSentence,
    ACSelectParagraph,
    ACSelectColumn,
    ACSelectRow,
    ACSelectTable,
    ACSelectObject,
    ACRedoRepeat,
    ACSort,
    ACSortAscending,
    ACSortDescending,
    ACFilter,
    ACSetClock,
    ACViewClock,
    ACSelectTimeZone,
    ACEditTimeZones,
    ACSetAlarm,
    ACClearAlarm,
    ACSnoozeAlarm,
    ACResetAlarm,
    ACSynchronize,
    ACSendReceive,
    ACSendTo,
    ACReply,
    ACReplyAll,
    ACForwardMsg,
    ACSend,
    ACAttachFile,
    ACUpload,
    ACDownloadSaveTargetAs,
    ACSetBorders,
    ACInsertRow,
    ACInsertColumn,
    ACInsertFile,
    ACInsertPicture,
    ACInsertObject,
    ACInsertSymbol,
    ACSaveAndClose,
    ACRename,
    ACMerge,
    ACSplit,
    ACDistributeHorizontally,
    ACDistributeVertically,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq, Clone)]
pub enum Inc {
    Up,
    Down,
}

impl_enum_to_tokens! {
    enum MouseButton: crate::keyboard::actions::MouseButton,
    enum MouseMovement: crate::keyboard::actions::MouseMovement,
    enum Inc: crate::utils::Inc,
    enum ConsumerKey: usbd_human_interface_device::page::Consumer,
    enum FirmwareAction: crate::keyboard::actions::FirmwareAction,
}

impl_enum_tuple_to_tokens! {
    enum Action: crate::keyboard::actions::Action { Led(led), Mouse(mouse), Consumer(consumer), Firmware(firmware) }
    enum LedAction: crate::keyboard::actions::LedAction { Cycle(inc), Brightness(inc) }
    enum MouseAction: crate::keyboard::actions::MouseAction { Click(button), Move(movement), Sensitivity(inc) }
}

#[cfg(test)]
pub mod tests {
    use proc_macro2::TokenStream;
    use crate::format::assert_tokens_eq;
    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!([
            { "Led": { "Cycle": "Up" } },
            { "Mouse": { "Move": "PanLeft" } },
            { "Consumer": "VolumeIncrement" },
            { "Firmware": "AllowBootloader" },
            { "Firmware": "InfiniteLoop" },
        ])
    }

    pub fn example_config() -> Vec<Action> {
        vec![
            Action::Led(LedAction::Cycle(Inc::Up)),
            Action::Mouse(MouseAction::Move(MouseMovement::PanLeft)),
            Action::Consumer(ConsumerKey::VolumeIncrement),
            Action::Firmware(FirmwareAction::AllowBootloader),
            Action::Firmware(FirmwareAction::InfiniteLoop),
        ]
    }

    pub fn example_code() -> TokenStream {
        quote! {
            [
                crate::keyboard::actions::Action::Led(
                    crate::keyboard::actions::LedAction::Cycle(
                        crate::utils::Inc::Up
                    )
                ),
                crate::keyboard::actions::Action::Mouse(
                    crate::keyboard::actions::MouseAction::Move(
                        crate::keyboard::actions::MouseMovement::PanLeft
                    )
                ),
                crate::keyboard::actions::Action::Consumer(
                    usbd_human_interface_device::page::Consumer::VolumeIncrement
                ),
                crate::keyboard::actions::Action::Firmware(
                    crate::keyboard::actions::FirmwareAction::AllowBootloader
                ),
                crate::keyboard::actions::Action::Firmware(
                    crate::keyboard::actions::FirmwareAction::InfiniteLoop
                ),
            ]
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let v: Vec<Action> = serde_json::from_value(example_json())?;
        assert_eq!(v, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        let q = example_config();
        assert_tokens_eq(quote! { [ #( #q ),* ] }, example_code())
    }
}
