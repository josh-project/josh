use std::collections::BTreeMap;

pub struct MockRuleset {
    pub id: String,
    pub name: String,
    pub enforcement: String,
    pub include_refs: Vec<String>,
    pub exclude_refs: Vec<String>,
    pub required_checks: Vec<String>,
}

pub struct GraphQLState {
    pub prs: Vec<MockPr>,
    pub reviews: BTreeMap<i64, Vec<(String, String)>>,
    pub maintainers: Vec<String>,
    pub rulesets: Vec<MockRuleset>,
    pub closed_prs: Vec<String>,
    pub comments: Vec<(String, String)>,
}

pub struct MockPr {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub head_ref_oid: String,
    pub head_ref_name: String,
    pub base_ref_oid: String,
    pub base_ref_name: String,
}
