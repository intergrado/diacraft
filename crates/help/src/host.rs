// The values of the host app, and nothing else: the one place that says what is DiaCraft's in this crate, so that the crate can move to
// craft-libs as `craft-help` with no change to its logic. A host changes this file and no other.
//
// This file is plain items and `//` comments, and no `use`, because `build.rs` includes it too (a build script cannot use the crate it builds), so the help tree's place is written once.

/// The sites a help link may open, written with no trailing slash: the website, the web app and the public repository. A link is allowed when it is one of these
/// exactly, or one of these followed by `/`, `?` or `#` (`url::allowed`). A host name that only ends with one of them, a user name in front of it and a port after it are not.
pub const WEB_PREFIXES: [&str; 3] = ["https://diacraft.dev", "https://diacraft.app", "https://github.com/intergrado/diacraft"];

/// The repository the feedback address names (`owner/name` on the code host), whose `issues/new` page takes the person's report. Its prefix is the last of [`WEB_PREFIXES`].
pub const FEEDBACK_REPOSITORY: &str = "intergrado/diacraft";

/// Where the help tree is, from the root of the repository: the topics, the tips table, the help map, the sections and the list of published ids.
pub const HELP_TREE: &str = "help";

/// Where the topics are, under [`HELP_TREE`]: `topics/<folder>/<id>.md`, a topic's file named by its id.
pub const TOPICS_DIR: &str = "topics";

/// The three tables, under [`HELP_TREE`].
pub const TIPS_FILE: &str = "tips.tsv";
pub const MAP_FILE: &str = "map.tsv";
pub const SECTIONS_FILE: &str = "sections.tsv";

/// Where the help catalogs are, from the root of the repository: `<locale>/<topic id>.tsv`, one folder per language.
pub const CATALOG_TREE: &str = "i18n/help";

/// From the folder of this crate's manifest to the root of the repository (`build.rs` reads the trees from there).
pub const REPOSITORY_ROOT: &str = "../..";

/// Topics the host makes at run time and no file holds (keys and the list of every command): their ids are known to the checker and to links, and they have no anchors.
pub const GENERATED_TOPICS: [&str; 2] = ["keyboard-shortcuts", "all-commands"];

/// The areas of the window a help map row may name (`window:<area>`).
pub const WINDOW_AREAS: [&str; 8] = ["canvas", "shapes", "sheet", "page_tabs", "status_bar", "title_bar", "search", "help"];
