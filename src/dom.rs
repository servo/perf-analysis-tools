use core::str;
use std::borrow::Borrow;

use html5ever::{
    local_name, namespace_url, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::TreeBuilderOpts,
    LocalName, ParseOpts, QualName,
};
use jane_eyre::eyre;
use markup5ever_rcdom::{Handle, RcDom};

pub struct Traverse(Vec<Handle>);

pub fn parse(mut input: &[u8]) -> eyre::Result<RcDom> {
    let options = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let context = QualName::new(None, ns!(html), local_name!("section"));
    let dom = html5ever::parse_fragment(RcDom::default(), options, context, vec![])
        .from_utf8()
        .read_from(&mut input)?;

    Ok(dom)
}

pub fn make_html_tag_name(name: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(name))
}

pub fn tendril_to_str(tendril: &StrTendril) -> eyre::Result<&str> {
    Ok(str::from_utf8(tendril.borrow())?)
}

impl Traverse {
    pub fn new(node: Handle) -> Self {
        Self(vec![node])
    }
}

impl Iterator for Traverse {
    type Item = Handle;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let node = self.0.remove(0);
        for kid in node.children.borrow().iter() {
            self.0.push(kid.clone());
        }

        Some(node)
    }
}
