//! Which namespace an element is in, so that an element of another vocabulary is never read as a VSDX one.
//!
//! `opc`'s tree keeps names as written (`x:note`) and resolves no namespace. A reader that matched by local name would take a third
//! party's `<x:Shape xmlns:x="urn:z">` for a shape (shows foreign elements among a shape's children). So
//! the reader keeps a [`Scope`]: the default namespace and the prefixes in force, each element's own `xmlns` declarations applied
//! when it is entered. An element is **VSDX's** when its namespace is either of the two the specs name (
//! `2012/main`, which every sample uses, and `2011/1/core`, which Microsoft's documents state) or **none at all**: a part with no
//! default namespace is read as VSDX's, which is more than the specs ask (a labelled choice: a hand-written part is read, and the
//! report counts such parts).
//!
//! **The scope is `diacraft-sheetxml`'s**: the walk of the frames and its cost bound (a prefix is answered from the nearest frame up,
//! and the answer kept in the frame the question was asked of) are written there, since the sheet codec asks the same question of every cell
//! and section. What is this crate's is the [`VsdxVocabulary`], which says which namespaces are VSDX's, and [`ScopeExt`], which names the
//! namespace of an element as an [`Ns`] and finds a relationship id.

use diacraft_opc::xml::El;
use diacraft_sheetxml::Vocabulary;

/// The namespace every sample uses.
pub(crate) const NS_2012: &str = "http://schemas.microsoft.com/office/visio/2012/main"; // brand-ok: a namespace the format defines
/// The namespace Microsoft's documents state. No sample uses it.
pub(crate) const NS_2011: &str = "http://schemas.microsoft.com/office/visio/2011/1/core"; // brand-ok: a namespace the format defines
/// The relationships namespace, which `r:id` is in.
pub(crate) const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// The markup compatibility namespace: `mc:Ignorable`, `mc:MustUnderstand`, `mc:AlternateContent`.
pub(crate) const NS_MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

/// The vocabulary of a part, or of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ns {
    /// `http://schemas.microsoft.com/office/visio/2012/main`.
    Main2012,
    /// `http://schemas.microsoft.com/office/visio/2011/1/core`.
    Core2011,
    /// No namespace at all.
    None,
    /// Any other.
    Other,
}

impl Ns {
    fn of(uri: Option<&str>) -> Ns {
        match uri {
            None | Some("") => Ns::None,
            Some(NS_2012) => Ns::Main2012,
            Some(NS_2011) => Ns::Core2011,
            Some(_) => Ns::Other,
        }
    }

    /// Whether the reader reads an element of this namespace as its own.
    pub(crate) fn is_ours(self) -> bool {
        !matches!(self, Ns::Other)
    }
}

/// The vocabulary a namespace URI names.
pub(crate) fn ns_of_uri(uri: &str) -> Ns {
    Ns::of(Some(uri))
}

/// The vocabulary of a part: the format's two namespaces, and none at all (a part with no default namespace). Any other namespace is not read.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VsdxVocabulary;

impl Vocabulary for VsdxVocabulary {
    fn reads(self, uri: Option<&str>) -> bool {
        Ns::of(uri).is_ours()
    }
}

/// The namespaces in force at an element, and the vocabulary they are read in.
pub(crate) type Scope = diacraft_sheetxml::Scope<VsdxVocabulary>;

/// What a [`Scope`] says in this crate's words.
pub(crate) trait ScopeExt {
    /// The namespace of `el`, which must be the element this scope was entered for. An unbound prefix is no namespace we know: `Ns::Other`.
    fn ns_of(&self, el: &El) -> Ns;

    /// The `r:id` of `el`: the attribute whose local name is `id` and whose prefix is bound to the relationships namespace. A file may
    /// bind another prefix to that namespace, and `r` to something else.
    fn rel_id<'a>(&self, el: &'a El) -> Option<&'a str>;

    /// Whether the attribute `name` is a relationship id here: its local name is `id` and its prefix is bound to the relationships namespace.
    fn is_rel_id_attr(&self, name: &str) -> bool;
}

impl ScopeExt for Scope {
    fn ns_of(&self, el: &El) -> Ns {
        self.classify(el, Ns::of)
    }

    fn rel_id<'a>(&self, el: &'a El) -> Option<&'a str> {
        el.attrs.iter().find(|(k, _)| self.is_rel_id_attr(k)).map(|(_, v)| v.as_str())
    }

    fn is_rel_id_attr(&self, name: &str) -> bool {
        self.is_attr_in(name, "id", NS_REL)
    }
}
