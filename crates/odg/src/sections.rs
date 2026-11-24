//! The model's names of the sections this crate builds or matches (SECNAME).
//!
//! A section's name is the model's own: the `N` of a `Section` of a file, one of [`diacraft_doc::SECTION_NAMES`] (`Property`, `Control`, `User`). **A formula spells
//! some of them another way** (`Prop.Cost`, `Controls.Row_1.X`: the calculator's table of spellings maps the formula's word to the section's name), and a reference
//! is no section's name. A section built or matched here by the formula's word is a section that no reference finds and that no other reader or writer of the
//! model matches: the shape data a stale link left the shape was never copied, and the handles the reader read were a section the formulas it wrote did not
//! resolve to.
//!
//! The names live here, once, and a test holds each to the model's list.

/// The section of a shape's data (`Prop.<row>` in a formula).
pub(crate) const PROPERTY: &str = "Property";

/// The section of a custom shape's handles (`Controls.<row>.X` in a formula).
pub(crate) const CONTROL: &str = "Control";

/// The section of a shape's user rows, and of the modifiers and equations the reader makes (`User.<row>` in a formula).
pub(crate) const USER: &str = "User";

#[cfg(test)]
mod tests {
    use diacraft_doc::SECTION_NAMES;

    use super::*;

    /// Each name is one the model lists, and the words a formula uses for two of them are not: a reader that built a section by such a word would build one no
    /// other part of the model knows.
    #[test]
    fn each_name_is_a_name_of_the_models_list_and_the_formulas_spellings_are_not() {
        for name in [PROPERTY, CONTROL, USER] {
            assert!(SECTION_NAMES.contains(&name), "{name}");
        }
        for spelled in ["Prop", "Controls"] {
            assert!(!SECTION_NAMES.contains(&spelled), "{spelled} is how a formula writes a section, not the section's name");
        }
    }
}
