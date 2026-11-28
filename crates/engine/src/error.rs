//! What an engine call can fail with.

/// The result of every engine call.
pub type Result<T> = std::result::Result<T, EngineError>;

/// Why a command did not run or did not finish. Whatever the reason, the session is as it was before
/// the command: a failed command changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// No command has this id, as `shown` makes it: escaped, and cut short when very long.
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    /// The command exists but cannot run now; `reason` says why (`no document open`).
    #[error("command `{cmd}` is not available right now: {reason}")]
    Disabled { cmd: String, reason: String },
    /// The command's parameters are wrong: an unknown key, a missing or ill-typed value, a value out of range.
    #[error("invalid parameters for `{cmd}`: {msg}")]
    BadParams { cmd: String, msg: String },
    /// The parameters were fine but the command could not do what they ask.
    #[error("`{cmd}` failed: {msg}")]
    Failed { cmd: String, msg: String },
    /// `file.open` could not open the file; `problem` says what kind of trouble it was, for a frontend to choose its words from, and `msg` has
    /// words for a person (and no text from the file, and not the path).
    #[error("`{cmd}` failed: {msg}")]
    CannotOpen { cmd: String, problem: OpenProblem, msg: String },
    /// A save would replace a file that has changed on the disk since the drawing was opened or last saved (something else wrote it), and `force`
    /// was not given. Nothing was written. A frontend asks the person, and runs the command again with `force: true` if the answer is to replace
    /// it; a program gets this as an error (of the M3 review). `msg` has words for a person, and not the path.
    #[error("`{cmd}` failed: {msg}")]
    ChangedOnDisk { cmd: String, msg: String },
    /// A save would put a file that holds a macro project into a type that cannot hold one (`.vsdx`, `.vssx` or `.vstx`), and `drop_macros` was not
    /// given. Nothing was written, and the project was not touched. A frontend asks the person to save as a type that allows macros or to leave the
    /// macros out, and runs the command again with `drop_macros: true` if the answer is the second; a program gets this as an error. `msg` has words
    /// for a person, and not the path.
    #[error("`{cmd}` failed: {msg}")]
    MacrosNotAllowed { cmd: String, msg: String },
    /// A bug: the command panicked or broke an engine rule. The session was put back as it was.
    #[error("internal error in `{cmd}`: {msg} (the document was left as it was before; please report this bug)")]
    Internal { cmd: String, msg: String },
}

/// What kind of trouble stopped a file from opening. A frontend that wants its own words reads this and not the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenProblem {
    /// The path names no file.
    NotFound,
    /// The system would not let the file be read.
    Denied,
    /// A folder, a device or a pipe.
    NotAFile,
    /// The file is larger than a file may be.
    FileTooLarge,
    /// There was not enough memory to read it.
    OutOfMemory,
    /// The system failed in some other way while reading it.
    Unreadable,
    /// It is not a drawing package: not a ZIP archive, or a package with no drawing in it.
    NotAPackage,
    /// It is a package past one of the reader's limits.
    TooBig,
    /// It is a package that is damaged, or has XML the reader refuses.
    Damaged,
    /// The automation roots keep this command from reading the path: it is outside the folders the person granted for reading, or none was granted (gaps A2, `file_access`).
    NotGranted,
}

impl OpenProblem {
    /// A fixed word for the problem, as it is written in a reply: `not_found`, `denied`, `not_a_file`, `file_too_large`, `out_of_memory`,
    /// `unreadable`, `not_a_package`, `too_big`, `damaged`, `not_granted`.
    pub fn word(self) -> &'static str {
        match self {
            OpenProblem::NotFound => "not_found",
            OpenProblem::Denied => "denied",
            OpenProblem::NotAFile => "not_a_file",
            OpenProblem::FileTooLarge => "file_too_large",
            OpenProblem::OutOfMemory => "out_of_memory",
            OpenProblem::Unreadable => "unreadable",
            OpenProblem::NotAPackage => "not_a_package",
            OpenProblem::TooBig => "too_big",
            OpenProblem::Damaged => "damaged",
            OpenProblem::NotGranted => "not_granted",
        }
    }
}

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.to_owned(), msg: msg.into() }
}

pub(crate) fn failed(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::Failed { cmd: cmd.to_owned(), msg: msg.into() }
}

pub(crate) fn cannot_open(cmd: &str, problem: OpenProblem, msg: impl Into<String>) -> EngineError {
    EngineError::CannotOpen { cmd: cmd.to_owned(), problem, msg: msg.into() }
}

pub(crate) fn changed_on_disk(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::ChangedOnDisk { cmd: cmd.to_owned(), msg: msg.into() }
}

pub(crate) fn macros_not_allowed(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::MacrosNotAllowed { cmd: cmd.to_owned(), msg: msg.into() }
}

pub(crate) fn internal(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::Internal { cmd: cmd.to_owned(), msg: msg.into() }
}

/// The most characters of a client's text an error message holds (see [`crate::shown`]).
const SHOWN_MOST: usize = 64;

/// `text` for an error message: [`crate::shown`] at 64 characters.
pub(crate) fn shown(text: &str) -> String {
    crate::text::shown(text, SHOWN_MOST)
}

/// The most characters of a panic's message an error holds. A plain message is far shorter, and [`crate::guard::catch_panic`]
/// has written the whole of it to the log.
const PANIC_MOST: usize = 400;

/// A panic's message for an error: [`crate::shown`] at 400 characters. The message can hold what a client sent (a command id,
/// a name, a path in a failed `unwrap` of a bug), so it reaches a reply, a status bar or a listing escaped like any other
/// client text.
pub(crate) fn shown_panic(msg: &str) -> String {
    crate::text::shown(msg, PANIC_MOST)
}
