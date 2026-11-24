//! The names of cells and sections, as the file spells them.
//!
//! **Exact case**: `PinX` and `pinx` are two names, and a name that differs from a
//! listed one only by case is an unknown cell, kept verbatim. These are not enums: a file may hold any name, and a closed list
//! would lose the ones it does not know. A name the catalogue lists shares the catalogue's allocation, so the hundreds of
//! thousands of `PinX` cells of a large drawing hold one string between them; any other name is allocated once.

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

use crate::catalogue::{SECTION_NAMES, known_cell_name};

#[derive(Clone)]
enum Repr {
    Static(&'static str),
    Shared(Arc<str>),
}

macro_rules! name_type {
    ($(#[$doc:meta])* $name:ident, $known:expr) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name(Repr);

        impl $name {
            /// The name `text`: the catalogue's own copy when it lists the name, else a new allocation.
            pub fn new(text: &str) -> $name {
                let known: fn(&str) -> Option<&'static str> = $known;
                match known(text) {
                    Some(name) => $name(Repr::Static(name)),
                    None => $name(Repr::Shared(Arc::from(text))),
                }
            }

            /// The name as text.
            pub fn as_str(&self) -> &str {
                match &self.0 {
                    Repr::Static(s) => s,
                    Repr::Shared(s) => s,
                }
            }

            /// Whether the name holds an allocation of its own (a name the catalogue does not list), which a reader charges for: the listed names
            /// share the catalogue's.
            pub fn is_allocated(&self) -> bool {
                matches!(self.0, Repr::Shared(_))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(self.as_str(), f)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        // Equality, order and hash are those of the text, so a map keyed by this type can be asked with a `&str`.
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.as_str() == other.as_str()
            }
        }
        impl Eq for $name {}
        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.as_str().cmp(other.as_str())
            }
        }
        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.as_str().hash(state);
            }
        }
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
        impl From<&str> for $name {
            fn from(text: &str) -> $name {
                $name::new(text)
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
    };
}

name_type!(
    /// The name of a cell, the `N` of a `Cell` element.
    CellName,
    known_cell_name
);

fn known_section_name(text: &str) -> Option<&'static str> {
    SECTION_NAMES.iter().find(|name| **name == text).copied()
}

name_type!(
    /// The name of a section, the `N` of a `Section` element.
    SectionName,
    known_section_name
);
