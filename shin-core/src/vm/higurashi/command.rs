//! Evaluated older-engine commands. Extra/unknown operands are retained until
//! their engine consumers are implemented; these are not Umineko commands.
use crate::{
    format::scenario::{
        higurashi::bytecode::HigurashiCommand,
        instruction_elements::{MessageId, Register, UntypedNumberArray},
    },
    vm::{IntoRuntimeForm, VmCtx},
};
use smallvec::SmallVec;

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HigurashiRuntimeCommand {
    EXIT {
        arg1: u8,
        arg2: i32,
    },
    SGET {
        dest: Register,
        slot: i32,
    },
    SSET {
        args: [i32; 2],
    },
    WAIT {
        allow_interrupt: u8,
        amount: i32,
    },
    MSGINIT {
        style: i32,
        updated_options: Option<[i32; 2]>,
    },
    MSGSET {
        msg_id: MessageId,
        auto_wait: u8,
        argument: i32,
        text: String,
    },
    MSGWAIT {
        signal: i32,
    },
    MSGSIGNAL {},
    MSGSYNC {
        args: [i32; 2],
    },
    MSGCLOSE {
        wait: u8,
    },
    MSGFACE {
        original_argument: Option<i32>,
    },
    LOGSET {
        text: String,
    },
    SELECT {
        base: u16,
        index: u16,
        dest: Register,
        visibility: i32,
        title: String,
        variants: Vec<String>,
    },
    WIPE {
        args: [i32; 3],
        params: UntypedNumberArray,
    },
    WIPEWAIT {},
    BGMPLAY {
        args: [i32; 4],
    },
    BGMSTOP {
        fade: i32,
    },
    BGMVOL {
        args: [i32; 2],
    },
    BGMWAIT {
        status: i32,
    },
    BGMSYNC {
        time: i32,
    },
    SEPLAY {
        args: [i32; 7],
    },
    SESTOP {
        args: [i32; 2],
    },
    SESTOPALL {
        fade: i32,
    },
    SEVOL {
        args: [i32; 3],
    },
    SEPAN {
        args: [i32; 3],
    },
    SEWAIT {
        args: [i32; 2],
    },
    SEONCE {
        args: [i32; 5],
    },
    VOICEPLAY {
        name: String,
        args: [i32; 2],
    },
    VOICESTOP {},
    VOICEWAIT {
        status: i32,
    },
    SAVEINFO {
        level: i32,
        info: String,
    },
    AUTOSAVE {},
    EVBEGIN {
        argument: i32,
    },
    EVEND {},
    TROPHY {
        id: i32,
    },
    LAYERINIT {
        id: i32,
    },
    LAYERLOAD {
        args: [i32; 3],
        params: UntypedNumberArray,
    },
    LAYERUNLOAD {
        args: [i32; 2],
    },
    LAYERCTRL {
        args: [i32; 2],
        params: UntypedNumberArray,
    },
    LAYERWAIT {
        id: i32,
        properties: SmallVec<[i32; 6]>,
    },
    LAYERBACK {
        argument: i32,
    },
    LAYERSWAP {
        args: [i32; 2],
    },
    LAYERSELECT {
        args: [i32; 2],
    },
    MOVIEWAIT {
        args: [i32; 2],
    },
    FEELICON {
        args: [i32; 5],
    },
    TIPSGET {
        ids: SmallVec<[i32; 6]>,
    },
    CHARSELECT {
        index: u16,
        dest: Register,
        argument: i32,
    },
    OTSUGET {
        argument: i32,
    },
    CHART {
        mode: u8,
        ids: SmallVec<[i32; 6]>,
    },
    SNRSEL {
        argument: i32,
    },
    KAKERA {},
    KAKERAGET {
        argument: i32,
        ids: SmallVec<[i32; 6]>,
    },
    QUIZ {
        dest: Register,
        args: [i32; 3],
    },
    FAKESELECT {},
    UNLOCK {
        kind: u8,
        ids: SmallVec<[i32; 6]>,
    },
    OpcodeDA {
        dest: Register,
        argument: i32,
    },
    OpcodeDB {
        args: [i32; 2],
    },
    DEBUGOUT {
        format: String,
        args: SmallVec<[i32; 6]>,
    },
}

impl HigurashiCommand {
    /// Evaluate every numeric operand against the current VM, including lists
    /// and sparse arrays. String decoding/fixups have already happened.
    pub fn into_runtime_form(self, ctx: &VmCtx) -> HigurashiRuntimeCommand {
        match self {
            Self::EXIT { arg1, arg2 } => HigurashiRuntimeCommand::EXIT {
                arg1,
                arg2: ctx.get_number(arg2),
            },
            Self::SGET { dest, slot } => HigurashiRuntimeCommand::SGET {
                dest,
                slot: ctx.get_number(slot),
            },
            Self::SSET { args } => HigurashiRuntimeCommand::SSET {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::WAIT {
                allow_interrupt,
                amount,
            } => HigurashiRuntimeCommand::WAIT {
                allow_interrupt,
                amount: ctx.get_number(amount),
            },
            Self::MSGINIT {
                style,
                updated_options,
            } => HigurashiRuntimeCommand::MSGINIT {
                style: ctx.get_number(style),
                updated_options: updated_options.map(|a| a.map(|n| ctx.get_number(n))),
            },
            Self::MSGSET {
                msg_id,
                auto_wait,
                argument,
                text,
            } => HigurashiRuntimeCommand::MSGSET {
                msg_id,
                auto_wait,
                argument: ctx.get_number(argument),
                text: text.0,
            },
            Self::MSGWAIT { signal } => HigurashiRuntimeCommand::MSGWAIT {
                signal: ctx.get_number(signal),
            },
            Self::MSGSIGNAL {} => HigurashiRuntimeCommand::MSGSIGNAL {},
            Self::MSGSYNC { args } => HigurashiRuntimeCommand::MSGSYNC {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::MSGCLOSE { wait } => HigurashiRuntimeCommand::MSGCLOSE { wait },
            Self::MSGFACE { original_argument } => HigurashiRuntimeCommand::MSGFACE {
                original_argument: original_argument.map(|n| ctx.get_number(n)),
            },
            Self::LOGSET { text } => HigurashiRuntimeCommand::LOGSET { text: text.0 },
            Self::SELECT {
                base,
                index,
                dest,
                visibility,
                title,
                variants,
            } => HigurashiRuntimeCommand::SELECT {
                base,
                index,
                dest,
                visibility: ctx.get_number(visibility),
                title: title.0,
                variants: variants.0,
            },
            Self::WIPE { args, params } => HigurashiRuntimeCommand::WIPE {
                args: args.map(|n| ctx.get_number(n)),
                params: params.into_runtime_form(ctx),
            },
            Self::WIPEWAIT {} => HigurashiRuntimeCommand::WIPEWAIT {},
            Self::BGMPLAY { args } => HigurashiRuntimeCommand::BGMPLAY {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::BGMSTOP { fade } => HigurashiRuntimeCommand::BGMSTOP {
                fade: ctx.get_number(fade),
            },
            Self::BGMVOL { args } => HigurashiRuntimeCommand::BGMVOL {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::BGMWAIT { status } => HigurashiRuntimeCommand::BGMWAIT {
                status: ctx.get_number(status),
            },
            Self::BGMSYNC { time } => HigurashiRuntimeCommand::BGMSYNC {
                time: ctx.get_number(time),
            },
            Self::SEPLAY { args } => HigurashiRuntimeCommand::SEPLAY {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::SESTOP { args } => HigurashiRuntimeCommand::SESTOP {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::SESTOPALL { fade } => HigurashiRuntimeCommand::SESTOPALL {
                fade: ctx.get_number(fade),
            },
            Self::SEVOL { args } => HigurashiRuntimeCommand::SEVOL {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::SEPAN { args } => HigurashiRuntimeCommand::SEPAN {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::SEWAIT { args } => HigurashiRuntimeCommand::SEWAIT {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::SEONCE { args } => HigurashiRuntimeCommand::SEONCE {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::VOICEPLAY { name, args } => HigurashiRuntimeCommand::VOICEPLAY {
                name: name.0,
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::VOICESTOP {} => HigurashiRuntimeCommand::VOICESTOP {},
            Self::VOICEWAIT { status } => HigurashiRuntimeCommand::VOICEWAIT {
                status: ctx.get_number(status),
            },
            Self::SAVEINFO { level, info } => HigurashiRuntimeCommand::SAVEINFO {
                level: ctx.get_number(level),
                info: info.0,
            },
            Self::AUTOSAVE {} => HigurashiRuntimeCommand::AUTOSAVE {},
            Self::EVBEGIN { argument } => HigurashiRuntimeCommand::EVBEGIN {
                argument: ctx.get_number(argument),
            },
            Self::EVEND {} => HigurashiRuntimeCommand::EVEND {},
            Self::TROPHY { id } => HigurashiRuntimeCommand::TROPHY {
                id: ctx.get_number(id),
            },
            Self::LAYERINIT { id } => HigurashiRuntimeCommand::LAYERINIT {
                id: ctx.get_number(id),
            },
            Self::LAYERLOAD { args, params } => HigurashiRuntimeCommand::LAYERLOAD {
                args: args.map(|n| ctx.get_number(n)),
                params: params.into_runtime_form(ctx),
            },
            Self::LAYERUNLOAD { args } => HigurashiRuntimeCommand::LAYERUNLOAD {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::LAYERCTRL { args, params } => HigurashiRuntimeCommand::LAYERCTRL {
                args: args.map(|n| ctx.get_number(n)),
                params: params.into_runtime_form(ctx),
            },
            Self::LAYERWAIT { id, properties } => HigurashiRuntimeCommand::LAYERWAIT {
                id: ctx.get_number(id),
                properties: properties.into_runtime_form(ctx),
            },
            Self::LAYERBACK { argument } => HigurashiRuntimeCommand::LAYERBACK {
                argument: ctx.get_number(argument),
            },
            Self::LAYERSWAP { args } => HigurashiRuntimeCommand::LAYERSWAP {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::LAYERSELECT { args } => HigurashiRuntimeCommand::LAYERSELECT {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::MOVIEWAIT { args } => HigurashiRuntimeCommand::MOVIEWAIT {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::FEELICON { args } => HigurashiRuntimeCommand::FEELICON {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::TIPSGET { ids } => HigurashiRuntimeCommand::TIPSGET {
                ids: ids.into_runtime_form(ctx),
            },
            Self::CHARSELECT {
                index,
                dest,
                argument,
            } => HigurashiRuntimeCommand::CHARSELECT {
                index,
                dest,
                argument: ctx.get_number(argument),
            },
            Self::OTSUGET { argument } => HigurashiRuntimeCommand::OTSUGET {
                argument: ctx.get_number(argument),
            },
            Self::CHART { mode, ids } => HigurashiRuntimeCommand::CHART {
                mode,
                ids: ids.into_runtime_form(ctx),
            },
            Self::SNRSEL { argument } => HigurashiRuntimeCommand::SNRSEL {
                argument: ctx.get_number(argument),
            },
            Self::KAKERA {} => HigurashiRuntimeCommand::KAKERA {},
            Self::KAKERAGET { argument, ids } => HigurashiRuntimeCommand::KAKERAGET {
                argument: ctx.get_number(argument),
                ids: ids.into_runtime_form(ctx),
            },
            Self::QUIZ { dest, args } => HigurashiRuntimeCommand::QUIZ {
                dest,
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::FAKESELECT {} => HigurashiRuntimeCommand::FAKESELECT {},
            Self::UNLOCK { kind, ids } => HigurashiRuntimeCommand::UNLOCK {
                kind,
                ids: ids.into_runtime_form(ctx),
            },
            Self::OpcodeDA { dest, argument } => HigurashiRuntimeCommand::OpcodeDA {
                dest,
                argument: ctx.get_number(argument),
            },
            Self::OpcodeDB { args } => HigurashiRuntimeCommand::OpcodeDB {
                args: args.map(|n| ctx.get_number(n)),
            },
            Self::DEBUGOUT { format, args } => HigurashiRuntimeCommand::DEBUGOUT {
                format: format.0,
                args: args.into_runtime_form(ctx),
            },
        }
    }
}

impl HigurashiRuntimeCommand {
    /// Commands with VM feedback must receive an explicit value from their
    /// consumer; tracing never invents a menu or persistent-state response.
    pub fn destination(&self) -> Option<Register> {
        match self {
            Self::SGET { dest, .. }
            | Self::SELECT { dest, .. }
            | Self::CHARSELECT { dest, .. }
            | Self::QUIZ { dest, .. }
            | Self::OpcodeDA { dest, .. } => Some(*dest),
            _ => None,
        }
    }
}
