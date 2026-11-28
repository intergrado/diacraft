//! A help tree read for use: the English topics, the tables, and the catalogs of the other languages.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::catalog::{Catalog, translate};
use crate::md::{Topic, parse};
use crate::tables::{Map, Sections, Tips};
use crate::{Code, MAX_BUNDLE_BYTES, MAX_TOPICS, Problem};

/// The texts of a help tree, as the host has them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Parts<'a> {
    /// The topics: file name (for the problems) and text.
    pub topics: &'a [(&'a str, &'a str)],
    /// `tips.tsv`.
    pub tips: &'a str,
    /// `map.tsv`.
    pub map: &'a str,
    /// `sections.tsv`.
    pub sections: &'a str,
    /// Catalogs: locale, topic id and text.
    pub catalogs: &'a [(&'a str, &'a str, &'a str)],
}

/// A help tree read: what could be read of it, and what was skipped.
#[derive(Debug, Default)]
pub struct Bundle {
    topics: BTreeMap<String, Topic>,
    catalogs: BTreeMap<(String, String), Catalog>,
    tips: Tips,
    map: Map,
    sections: Sections,
    problems: Vec<(String, Problem)>,
}

impl Bundle {
    /// The English topic `id`.
    pub fn topic(&self, id: &str) -> Option<&Topic> {
        self.topics.get(id)
    }

    /// The ids of the topics, in alphabetical order.
    pub fn ids(&self) -> Vec<&str> {
        self.topics.keys().map(String::as_str).collect()
    }

    /// The topic `id` in `locale` (a tag such as `de` or `pt-br`): the English topic with the catalog's translations in it, or the English topic when there is no catalog.
    pub fn translated(&self, id: &str, locale: &str) -> Option<Topic> {
        let topic = self.topics.get(id)?;
        Some(match self.catalogs.get(&(locale.to_owned(), id.to_owned())) {
            Some(catalog) => translate(topic, catalog).topic,
            None => topic.clone(),
        })
    }

    /// The catalog of the topic `id` in `locale`.
    pub fn catalog(&self, locale: &str, id: &str) -> Option<&Catalog> {
        self.catalogs.get(&(locale.to_owned(), id.to_owned()))
    }

    /// The tips table.
    pub fn tips(&self) -> &Tips {
        &self.tips
    }

    /// The help map.
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The sections of the help home.
    pub fn sections(&self) -> &Sections {
        &self.sections
    }

    /// What was skipped when the tree was read, each with the file it was in. The host logs these; a checker fails on any.
    pub fn problems(&self) -> &[(String, Problem)] {
        &self.problems
    }
}

/// Reads a help tree from texts. A topic that cannot be read is skipped with its problem, at most [`MAX_TOPICS`] topics and [`MAX_BUNDLE_BYTES`] of them are kept, and a row
/// of a table or a catalog that breaks a rule is skipped: the rest is read.
pub fn from_parts(parts: &Parts<'_>) -> Bundle {
    let mut bundle = Bundle::default();
    let mut bytes = 0usize;
    for (file, text) in parts.topics {
        if bundle.topics.len() >= MAX_TOPICS || bytes.saturating_add(text.len()) > MAX_BUNDLE_BYTES {
            bundle.problems.push(((*file).to_owned(), Problem::new(0, Code::BundleTooLarge, "")));
            break;
        }
        bytes += text.len();
        match parse(text) {
            Err(problem) => bundle.problems.push(((*file).to_owned(), problem)),
            Ok(parsed) => {
                bundle.problems.extend(parsed.problems.into_iter().map(|p| ((*file).to_owned(), p)));
                match bundle.topics.entry(parsed.topic.header.id.clone()) {
                    Entry::Occupied(taken) => bundle.problems.push(((*file).to_owned(), Problem::new(0, Code::TopicDuplicate, taken.key()))),
                    Entry::Vacant(free) => {
                        free.insert(parsed.topic);
                    }
                }
            }
        }
    }
    let (tips, problems) = Tips::parse(parts.tips);
    bundle.tips = tips;
    bundle.problems.extend(problems.into_iter().map(|p| ("tips.tsv".to_owned(), p)));
    let (map, problems) = Map::parse(parts.map);
    bundle.map = map;
    bundle.problems.extend(problems.into_iter().map(|p| ("map.tsv".to_owned(), p)));
    let (sections, problems) = Sections::parse(parts.sections);
    bundle.sections = sections;
    bundle.problems.extend(problems.into_iter().map(|p| ("sections.tsv".to_owned(), p)));
    for (locale, id, text) in parts.catalogs {
        let (catalog, problems) = Catalog::parse(text, is_rtl(locale));
        bundle.problems.extend(problems.into_iter().map(|p| (format!("{locale}/{id}.tsv"), p)));
        bundle.catalogs.insert(((*locale).to_owned(), (*id).to_owned()), catalog);
    }
    bundle
}

/// Whether a language is written right to left, by its tag's primary subtag.
fn is_rtl(locale: &str) -> bool {
    let primary = locale.split(['-', '_']).next().unwrap_or_default();
    ["ar", "he", "fa", "ur"].iter().any(|r| primary.eq_ignore_ascii_case(r))
}
