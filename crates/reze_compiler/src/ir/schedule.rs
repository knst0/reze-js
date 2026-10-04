use oxc_syntax::symbol::SymbolId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueMode {
    Once,
    Tracked,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemberKind {
    Prop,
    Insert,
    Link,
    Late,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchedMember {
    pub kind: MemberKind,
    pub index: usize,
}

pub struct EffectGroup {
    pub deps: Vec<SymbolId>,
    pub members: Vec<SchedMember>,
}

pub struct Schedule {
    pub immediate: Vec<SchedMember>,
    pub children: Vec<SchedMember>,
    pub post_children: Vec<SchedMember>,
    pub effects: Vec<EffectGroup>,
}

impl Schedule {
    pub fn empty() -> Self {
        Self {
            immediate: Vec::new(),
            children: Vec::new(),
            post_children: Vec::new(),
            effects: Vec::new(),
        }
    }
}
