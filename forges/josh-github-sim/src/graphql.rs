use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use indexmap::IndexMap;
use juniper::{
    DefaultScalarValue, EmptySubscription, ID, InputValue, Scalar, ScalarValue, Variables,
    WrongInputScalarTypeError, graphql_object,
};
use serde::Deserialize;

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

struct Context {
    repos: HashMap<(String, String), PathBuf>,
    state: Arc<Mutex<GraphQLState>>,
}

impl juniper::Context for Context {}

struct Query;

#[derive(juniper::GraphQLEnum)]
enum PullRequestState {
    OPEN,
    CLOSED,
    MERGED,
}

#[derive(Clone, Debug, juniper::GraphQLScalar)]
#[graphql(parse_token(String))]
struct GitObjectID(String);

impl GitObjectID {
    fn to_output(&self) -> &str {
        &self.0
    }

    fn from_input<S: ScalarValue>(v: &Scalar<S>) -> Result<Self, WrongInputScalarTypeError<'_, S>> {
        v.try_to_string()
            .map(GitObjectID)
            .ok_or_else(|| WrongInputScalarTypeError {
                type_name: arcstr::literal!("String"),
                input: &**v,
            })
    }
}

#[derive(juniper::GraphQLEnum)]
enum RepositoryRuleType {
    RequiredStatusChecks,
}

#[derive(juniper::GraphQLInputObject)]
struct AddCommentInput {
    subject_id: ID,
    body: String,
}

#[derive(juniper::GraphQLInputObject)]
struct ClosePullRequestInput {
    pull_request_id: ID,
}

#[graphql_object(context = Context)]
impl Query {
    async fn repository(owner: String, name: String, context: &Context) -> Option<Repository> {
        let key = (owner.clone(), name.clone());
        if context.repos.contains_key(&key) {
            Some(Repository { owner, name })
        } else {
            None
        }
    }

    fn node(id: ID, context: &Context) -> Option<RepositoryRuleset> {
        let state = context.state.lock().unwrap();
        state
            .rulesets
            .iter()
            .find(|rs| rs.id == id.to_string())
            .map(|rs| RepositoryRuleset {
                id: rs.id.clone(),
                name: rs.name.clone(),
                enforcement: rs.enforcement.clone(),
                target: "BRANCH".to_string(),
                conditions: RulesetConditions {
                    ref_name: RefNameCondition {
                        include: rs.include_refs.clone(),
                        exclude: rs.exclude_refs.clone(),
                    },
                },
                required_checks: rs.required_checks.clone(),
            })
    }
}

struct Repository {
    owner: String,
    name: String,
}

#[graphql_object(context = Context)]
impl Repository {
    fn name_with_owner(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn default_branch_ref(&self) -> DefaultBranchRef {
        DefaultBranchRef
    }

    fn pull_requests(
        &self,
        first: i32,
        _after: Option<String>,
        states: Option<Vec<PullRequestState>>,
        context: &Context,
    ) -> PullRequestConnection {
        let state = context.state.lock().unwrap();
        let all_prs: Vec<&MockPr> = state
            .prs
            .iter()
            .filter(|_pr| {
                if let Some(ref states) = states {
                    states.iter().any(|s| matches!(s, PullRequestState::OPEN))
                } else {
                    true
                }
            })
            .collect();
        let total_count = all_prs.len() as i32;
        let nodes: Vec<PullRequest> = all_prs
            .into_iter()
            .take(first as usize)
            .map(|pr| PullRequest {
                id: pr.node_id.clone(),
                number: pr.number as i32,
                title: pr.title.clone(),
                head_ref_oid: pr.head_ref_oid.clone(),
                head_ref_name: pr.head_ref_name.clone(),
                base_ref_oid: pr.base_ref_oid.clone(),
                base_ref_name: pr.base_ref_name.clone(),
            })
            .collect();
        PullRequestConnection { nodes, total_count }
    }

    fn pull_request(&self, number: i32, context: &Context) -> Option<PullRequest> {
        let state = context.state.lock().unwrap();
        state
            .prs
            .iter()
            .find(|pr| pr.number == number as i64)
            .map(|pr| PullRequest {
                id: pr.node_id.clone(),
                number: pr.number as i32,
                title: pr.title.clone(),
                head_ref_oid: pr.head_ref_oid.clone(),
                head_ref_name: pr.head_ref_name.clone(),
                base_ref_oid: pr.base_ref_oid.clone(),
                base_ref_name: pr.base_ref_name.clone(),
            })
    }

    fn collaborators(
        &self,
        first: i32,
        _after: Option<String>,
        context: &Context,
    ) -> CollaboratorConnection {
        let state = context.state.lock().unwrap();
        let edges: Vec<CollaboratorEdge> = state
            .maintainers
            .iter()
            .take(first as usize)
            .map(|login| CollaboratorEdge {
                permission: "WRITE".to_string(),
                node: User {
                    login: login.clone(),
                },
            })
            .collect();
        CollaboratorConnection { edges }
    }

    fn rulesets(
        &self,
        first: i32,
        _include_parents: Option<bool>,
        context: &Context,
    ) -> RulesetConnection {
        let state = context.state.lock().unwrap();
        let nodes: Vec<RepositoryRuleset> = state
            .rulesets
            .iter()
            .take(first as usize)
            .map(|rs| RepositoryRuleset {
                id: rs.id.clone(),
                name: rs.name.clone(),
                enforcement: rs.enforcement.clone(),
                target: "BRANCH".to_string(),
                conditions: RulesetConditions {
                    ref_name: RefNameCondition {
                        include: rs.include_refs.clone(),
                        exclude: rs.exclude_refs.clone(),
                    },
                },
                required_checks: rs.required_checks.clone(),
            })
            .collect();
        RulesetConnection { nodes }
    }

    fn object(&self, oid: GitObjectID, context: &Context) -> Option<GitObject> {
        let oid = oid.0;
        let state = context.state.lock().unwrap();
        let has_matching_pr = state
            .prs
            .iter()
            .any(|pr| pr.head_ref_oid == oid || pr.base_ref_oid == oid);
        if has_matching_pr {
            let oid_clone = oid.clone();
            Some(GitObject {
                oid,
                associated_prs_nodes: state
                    .prs
                    .iter()
                    .filter(|pr| pr.head_ref_oid == oid_clone)
                    .map(|pr| PullRequest {
                        id: pr.node_id.clone(),
                        number: pr.number as i32,
                        title: pr.title.clone(),
                        head_ref_oid: pr.head_ref_oid.clone(),
                        head_ref_name: pr.head_ref_name.clone(),
                        base_ref_oid: pr.base_ref_oid.clone(),
                        base_ref_name: pr.base_ref_name.clone(),
                    })
                    .collect(),
            })
        } else {
            None
        }
    }
}

struct DefaultBranchRef;

#[graphql_object(context = Context)]
impl DefaultBranchRef {
    fn name(&self) -> &str {
        "main"
    }
}

struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[graphql_object(context = Context)]
impl PageInfo {
    fn has_next_page(&self) -> bool {
        self.has_next_page
    }
    fn end_cursor(&self) -> Option<&str> {
        self.end_cursor.as_deref()
    }
}

#[derive(Clone)]
struct PullRequest {
    id: String,
    number: i32,
    title: String,
    head_ref_oid: String,
    head_ref_name: String,
    base_ref_oid: String,
    base_ref_name: String,
}

#[graphql_object(context = Context)]
impl PullRequest {
    fn id(&self) -> &str {
        &self.id
    }
    fn number(&self) -> i32 {
        self.number
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn head_ref_oid(&self) -> &str {
        &self.head_ref_oid
    }
    fn head_ref_name(&self) -> &str {
        &self.head_ref_name
    }
    fn base_ref_oid(&self) -> &str {
        &self.base_ref_oid
    }
    fn base_ref_name(&self) -> &str {
        &self.base_ref_name
    }

    fn reviews(&self, first: i32, _after: Option<String>, context: &Context) -> ReviewConnection {
        let state = context.state.lock().unwrap();
        let nodes: Vec<Review> = state
            .reviews
            .get(&(self.number as i64))
            .map(|review_list| {
                review_list
                    .iter()
                    .map(|(login, review_state)| Review {
                        author: User {
                            login: login.clone(),
                        },
                        state: review_state.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let nodes = nodes.into_iter().take(first as usize).collect();
        ReviewConnection { nodes }
    }
}

struct User {
    login: String,
}

#[graphql_object(context = Context)]
impl User {
    fn login(&self) -> &str {
        &self.login
    }
}

struct Review {
    author: User,
    state: String,
}

#[graphql_object(context = Context)]
impl Review {
    fn author(&self) -> &User {
        &self.author
    }
    fn state(&self) -> &str {
        &self.state
    }
}

struct ReviewConnection {
    nodes: Vec<Review>,
}

#[graphql_object(context = Context)]
impl ReviewConnection {
    fn nodes(&self) -> &[Review] {
        &self.nodes
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}

struct PullRequestConnection {
    nodes: Vec<PullRequest>,
    total_count: i32,
}

#[graphql_object(context = Context)]
impl PullRequestConnection {
    fn nodes(&self) -> &[PullRequest] {
        &self.nodes
    }
    fn total_count(&self) -> i32 {
        self.total_count
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}

struct GitObject {
    oid: String,
    associated_prs_nodes: Vec<PullRequest>,
}

#[graphql_object(context = Context, name = "Commit")]
impl GitObject {
    fn oid(&self) -> &str {
        &self.oid
    }

    fn associated_pull_requests(
        &self,
        _first: i32,
        _states: Option<Vec<PullRequestState>>,
    ) -> AssociatedPullRequestConnection {
        AssociatedPullRequestConnection {
            nodes: self.associated_prs_nodes.clone(),
        }
    }
}

struct AssociatedPullRequestConnection {
    nodes: Vec<PullRequest>,
}

#[graphql_object(context = Context)]
impl AssociatedPullRequestConnection {
    fn nodes(&self) -> &[PullRequest] {
        &self.nodes
    }
}

struct CollaboratorEdge {
    permission: String,
    node: User,
}

#[graphql_object(context = Context, name = "RepositoryCollaboratorEdge")]
impl CollaboratorEdge {
    fn permission(&self) -> &str {
        &self.permission
    }
    fn node(&self) -> &User {
        &self.node
    }
}

struct CollaboratorConnection {
    edges: Vec<CollaboratorEdge>,
}

#[graphql_object(context = Context, name = "RepositoryCollaboratorConnection")]
impl CollaboratorConnection {
    fn edges(&self) -> &[CollaboratorEdge] {
        &self.edges
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}

struct RefNameCondition {
    include: Vec<String>,
    exclude: Vec<String>,
}

#[graphql_object(context = Context)]
impl RefNameCondition {
    fn include(&self) -> &[String] {
        &self.include
    }
    fn exclude(&self) -> &[String] {
        &self.exclude
    }
}

struct RulesetConditions {
    ref_name: RefNameCondition,
}

#[graphql_object(context = Context)]
impl RulesetConditions {
    fn ref_name(&self) -> &RefNameCondition {
        &self.ref_name
    }
}

struct RepositoryRuleset {
    id: String,
    name: String,
    enforcement: String,
    target: String,
    conditions: RulesetConditions,
    required_checks: Vec<String>,
}

#[graphql_object(context = Context)]
impl RepositoryRuleset {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn enforcement(&self) -> &str {
        &self.enforcement
    }
    fn target(&self) -> &str {
        &self.target
    }
    fn conditions(&self) -> &RulesetConditions {
        &self.conditions
    }
    fn rules(&self, _first: i32, _type: Option<RepositoryRuleType>) -> RulesConnection {
        if self.required_checks.is_empty() {
            return RulesConnection { nodes: vec![] };
        }
        let nodes = vec![RepositoryRule {
            rule_type: "REQUIRED_STATUS_CHECKS".to_string(),
            parameters: RuleParameters {
                required_status_checks: self
                    .required_checks
                    .iter()
                    .map(|context_str| RequiredStatusCheck {
                        context: context_str.clone(),
                        integration_id: None,
                    })
                    .collect(),
                strict_required_status_checks_policy: false,
            },
        }];
        RulesConnection { nodes }
    }
}

struct RulesConnection {
    nodes: Vec<RepositoryRule>,
}

#[graphql_object(context = Context)]
impl RulesConnection {
    fn nodes(&self) -> &[RepositoryRule] {
        &self.nodes
    }
}

// Placeholder types for RepositoryRule — fleshed out in TASK_6
struct RepositoryRule {
    rule_type: String,
    parameters: RuleParameters,
}

#[graphql_object(context = Context)]
impl RepositoryRule {
    #[graphql(name = "type")]
    fn rule_type(&self) -> &str {
        &self.rule_type
    }
    fn parameters(&self) -> &RuleParameters {
        &self.parameters
    }
}

struct RuleParameters {
    required_status_checks: Vec<RequiredStatusCheck>,
    strict_required_status_checks_policy: bool,
}

#[graphql_object(context = Context, name = "RequiredStatusChecksParameters")]
impl RuleParameters {
    fn required_status_checks(&self) -> &[RequiredStatusCheck] {
        &self.required_status_checks
    }
    fn strict_required_status_checks_policy(&self) -> bool {
        self.strict_required_status_checks_policy
    }
}

struct RequiredStatusCheck {
    context: String,
    integration_id: Option<i32>,
}

#[graphql_object(context = Context)]
impl RequiredStatusCheck {
    fn context(&self) -> &str {
        &self.context
    }
    fn integration_id(&self) -> Option<i32> {
        self.integration_id
    }
}

struct RulesetConnection {
    nodes: Vec<RepositoryRuleset>,
}

#[graphql_object(context = Context)]
impl RulesetConnection {
    fn nodes(&self) -> &[RepositoryRuleset] {
        &self.nodes
    }
}

struct Mutation;

#[graphql_object(context = Context)]
impl Mutation {
    fn close_pull_request(
        input: ClosePullRequestInput,
        context: &Context,
    ) -> ClosePullRequestPayload {
        let pull_request_node_id = input.pull_request_id.to_string();
        let mut state = context.state.lock().unwrap();
        state.closed_prs.push(pull_request_node_id.clone());
        state.prs.retain(|pr| pr.node_id != pull_request_node_id);
        ClosePullRequestPayload {
            pull_request: ClosePullRequestResult {
                id: pull_request_node_id,
            },
        }
    }

    fn add_comment(input: AddCommentInput, context: &Context) -> AddCommentPayload {
        let subject_id = input.subject_id.to_string();
        let body = input.body;
        context
            .state
            .lock()
            .unwrap()
            .comments
            .push((subject_id, body));
        AddCommentPayload {
            client_mutation_id: None,
        }
    }
}

struct ClosePullRequestPayload {
    pull_request: ClosePullRequestResult,
}

#[graphql_object(context = Context)]
impl ClosePullRequestPayload {
    fn pull_request(&self) -> &ClosePullRequestResult {
        &self.pull_request
    }
}

struct ClosePullRequestResult {
    id: String,
}

#[graphql_object(context = Context)]
impl ClosePullRequestResult {
    fn id(&self) -> &str {
        &self.id
    }
}

struct AddCommentPayload {
    client_mutation_id: Option<String>,
}

#[graphql_object(context = Context)]
impl AddCommentPayload {
    fn client_mutation_id(&self) -> Option<&str> {
        self.client_mutation_id.as_deref()
    }
}

type Schema = juniper::RootNode<Query, Mutation, EmptySubscription<Context>>;

fn create_schema() -> Schema {
    Schema::new(Query, Mutation, EmptySubscription::new())
}

#[derive(Deserialize)]
struct GraphQLPayload {
    query: String,
    #[serde(rename = "operationName")]
    operation_name: Option<String>,
    variables: Option<serde_json::Value>,
}

fn variables_to_juniper(json: &serde_json::Value) -> Variables<DefaultScalarValue> {
    let mut vars = Variables::new();
    if let Some(obj) = json.as_object() {
        for (key, value) in obj.iter() {
            vars.insert(key.clone(), to_input_value(value));
        }
    }
    vars
}

fn to_input_value(json: &serde_json::Value) -> InputValue<DefaultScalarValue> {
    match json {
        serde_json::Value::Null => InputValue::Null,
        serde_json::Value::Bool(b) => InputValue::scalar(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                InputValue::scalar(i as i32)
            } else if let Some(f) = n.as_f64() {
                InputValue::scalar(f)
            } else {
                InputValue::Null
            }
        }
        serde_json::Value::String(s) => InputValue::scalar(s.clone()),
        serde_json::Value::Array(arr) => InputValue::list(arr.iter().map(to_input_value).collect()),
        serde_json::Value::Object(obj) => {
            let map: IndexMap<String, InputValue<DefaultScalarValue>> = obj
                .iter()
                .map(|(k, v)| (k.clone(), to_input_value(v)))
                .collect();
            InputValue::object(map)
        }
    }
}

struct GraphQLError {
    body: serde_json::Value,
}

impl GraphQLError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            body: serde_json::json!({"errors": [{"message": message.into()}]}),
        }
    }
}

impl IntoResponse for GraphQLError {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self.body)).into_response()
    }
}

pub(crate) async fn handle_graphql_request(
    repos: &HashMap<(String, String), PathBuf>,
    state: &Arc<Mutex<GraphQLState>>,
    request: axum::extract::Request,
) -> Response<Body> {
    let Json(payload) = match Json::<GraphQLPayload>::from_request(request, &()).await {
        Ok(p) => p,
        Err(e) => return GraphQLError::from_message(e.to_string()).into_response(),
    };

    let variables = variables_to_juniper(&payload.variables.unwrap_or_default());

    let context = Context {
        repos: repos.clone(),
        state: state.clone(),
    };
    let schema = create_schema();

    let result = juniper::execute(
        &payload.query,
        payload.operation_name.as_deref(),
        &schema,
        &variables,
        &context,
    )
    .await;

    (
        StatusCode::OK,
        Json(juniper::http::GraphQLResponse::from_result(result)),
    )
        .into_response()
}
