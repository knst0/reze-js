/// Exports of the framework packages that the compiler lowers or rewrites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Signal,
    Computed,
    Store,
    Action,
    Effect,
    OnCleanup,
    Untrack,
    Show,
    Switch,
    Match,
    For,
    Repeat,
    Loading,
    Errored,
    Portal,
    Dynamic,
    CreateContext,
    UseContext,
    CreateUniqueId,
    Loader,
    Mutation,
    GuardRoute,
    Defer,
    UseSubmission,
    UseParams,
    UseLocation,
    UseSearchParams,
    UseLinkState,
    BuildPaths,
}

impl Api {
    pub fn from_import(package: &str, name: &str) -> Option<Api> {
        use Api::*;
        Some(match (package, name) {
            ("reze-js", "signal") => Signal,
            ("reze-js", "computed") => Computed,
            ("reze-js", "store") => Store,
            ("reze-js", "action") => Action,
            ("reze-js", "effect") => Effect,
            ("reze-js", "onCleanup") => OnCleanup,
            ("reze-js", "untrack") => Untrack,
            ("reze-js", "Show") => Show,
            ("reze-js", "Switch") => Switch,
            ("reze-js", "Match") => Match,
            ("reze-js", "For") => For,
            ("reze-js", "Repeat") => Repeat,
            ("reze-js", "Loading") => Loading,
            ("reze-js", "Errored") => Errored,
            ("reze-js", "Portal") => Portal,
            ("reze-js", "dynamic") => Dynamic,
            ("reze-js", "createContext") => CreateContext,
            ("reze-js", "useContext") => UseContext,
            ("reze-js", "createUniqueId") => CreateUniqueId,
            ("reze-js/server", "loader") => Loader,
            ("reze-js/server", "mutation") => Mutation,
            ("reze-js/server", "guardRoute") => GuardRoute,
            ("reze-js/server", "defer") => Defer,
            ("reze-js/server", "useSubmission") => UseSubmission,
            ("reze-js", "useParams") => UseParams,
            ("reze-js", "useLocation") => UseLocation,
            ("reze-js", "useSearchParams") => UseSearchParams,
            ("reze-js", "useLinkState") => UseLinkState,
            ("reze-js", "buildPaths") => BuildPaths,
            _ => return None,
        })
    }

    /// Compiler components: `<Show>` and the rest, which exist only as JSX tags.
    pub fn is_tag(self) -> bool {
        matches!(
            self,
            Api::Show
                | Api::Switch
                | Api::Match
                | Api::For
                | Api::Repeat
                | Api::Loading
                | Api::Errored
                | Api::Portal
        )
    }

    /// Lowered away on every target, so the import has no runtime value.
    pub fn is_syntax(self) -> bool {
        !matches!(
            self,
            Api::CreateContext
                | Api::Loader
                | Api::Mutation
                | Api::GuardRoute
                | Api::Defer
                | Api::BuildPaths
        )
    }
}
