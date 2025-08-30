import './App.scss';
import {FileList} from './FileBrowser';

import {useEffect} from 'react';
import {None, Option} from 'tsoption';
import {
    LibraryOutline,
    GitCommitOutline,
    GitPullRequestOutline,
    SearchOutline
} from 'react-ionicons'

import {
    BrowserRouter,
    createSearchParams,
    Link,
    Navigate,
    Route,
    Routes,
    useNavigate,
    useSearchParams
} from 'react-router-dom';

import {RepoSelector} from './RepoSelector';
import {NavigateCallback, NavigateTarget, NavigateTargetType} from "./Navigation";
import {match} from "ts-pattern";
import {FileViewer} from "./FileViewer";
import {DiffViewer} from "./DiffViewer";
import {ChangeViewer} from "./ChangeViewer";
import {HistoryList} from "./History";
import {ChangesList} from "./Changes";
import {Breadcrumbs} from "./Breadcrumbs";
import {DEFAULT_FILTER} from "./Josh";

function useTitleEffect(title: string) {
    useEffect(() => {
        document.title = title
    });
}

function useNavigateCallback(): NavigateCallback {
    const navigate = useNavigate()
    return (targetType: NavigateTargetType, target: NavigateTarget) => {
        const params = {
            repo:   target.repo,
            path:   target.path,
            filter: target.filter,
            rev:    target.rev,
        }

        const pathname = match(targetType)
            .with(NavigateTargetType.History, () => '/history')
            .with(NavigateTargetType.Directory, () => '/browse')
            .with(NavigateTargetType.Change, () => '/change')
            .with(NavigateTargetType.File, () => '/view')
            .with(NavigateTargetType.Diff, () => '/diff')
            .run()

        navigate({
            pathname: pathname,
            search: `?${createSearchParams(params)}`
        })
    }
}

function useGetSearchParam() {
    let [ searchParams ] = useSearchParams()

    return (key: string): Option<string> => {
        let value = searchParams.get(key)

        if (value === null) {
            return new None()
        }

        return Option.of(value)
    }
}

function useStrictGetSearchParam() {
    const param = useGetSearchParam()

    return (key: string): string => {
        const value = param(key)

        if (value.isEmpty()) {
            throw new Error(`Search param ${key} was not provided`)
        } else {
            return value.getOrElse('')
        }
    }
}

function Select() {
    const param = useGetSearchParam()

    useTitleEffect(`Select repo - Josh`)

    const filter = param('filter').flatMap(value => {
        if (value === DEFAULT_FILTER) {
            return new None<string>()
        } else {
            return Option.of(value)
        }
    })

    return <div className={'ui-modal-container'}>
        <div className={'ui-modal'}>
            <RepoSelector
                repo={param('repo')}
                filter={filter}
                navigateCallback={useNavigateCallback()}
            />
        </div>
    </div>
}

function TopNav(props: { repo: string, filter: string, page: string }) {
    const [params, _] = useSearchParams()

    return <div className={'now-browsing'}>
        <div className="logo">
          <span className="now-browsing-name">
              <Link to={`/history?${createSearchParams(params)}`}>
                  {props.repo}{props.filter !== DEFAULT_FILTER && props.filter}
              </Link>
          </span>
        </div>
        <div className="current-page">
            <span>{props.page}</span>
        </div>
    </div>
}

function Browse() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`/${param('path')} - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
            page={param('rev')}
            filter={param('filter')} />

        <Breadcrumbs
            repo={param('repo')}
            path={param('path')}
            filter={param('filter')}
            rev={param('rev')}
            navigateCallback={useNavigateCallback()} />

        <FileList
            repo={param('repo')}
            path={param('path')}
            filter={param('filter')}
            rev={param('rev')}
            navigateCallback={useNavigateCallback()}
        />
    </div>
}

function ChangeView() {
    const param = useStrictGetSearchParam()

    useEffect(() => {
        document.title = `/${param('path')} - ${param('repo')} - Josh`
    });

    return <div>
        <TopNav
            repo={param('repo')} 
            page="change"
            filter={param('filter')} />

        <ChangeViewer
            repo={param('repo')}
            filter={param('filter')}
            rev={param('rev')}
            navigateCallback={useNavigateCallback()}
        />
    </div>
}

function History() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`History - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
            page={param('rev')}
            filter={param('filter')} />

        <HistoryList
            repo={param('repo')}
            filter={param('filter')}
            rev={param('rev')}
            navigateCallback={useNavigateCallback()}
        />
    </div>
}

function Changes() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`Changes - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
            page="changes"
            filter={param('filter')} />

        <ChangesList
            repo={param('repo')}
            filter={param('filter')}
            navigateCallback={useNavigateCallback()}
        />
    </div>
}

function Search() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`Search - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
            page="search"
            filter={param('filter')} />

        <SearchResults
            repo={param('repo')}
            filter={param('filter')}
            searchstr={param('q')}
            navigateCallback={useNavigateCallback()}
        />
    </div>
}


function View() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`${param('path')} - ${param('repo')} - Josh`)

    return (
        <div>
            <TopNav
                repo={param('repo')} 
                page="file"
                filter={param('filter')} />

            <Breadcrumbs
                repo={param('repo')}
                path={param('path')}
                filter={param('filter')}
                rev={param('rev')}
                navigateCallback={useNavigateCallback()} />

            <FileViewer
                repo={param('repo')}
                path={param('path')}
                filter={param('filter')}
                rev={param('rev')}
                navigateCallback={useNavigateCallback()}
            />
        </div>
    )
}

function DiffView() {
    const param = useStrictGetSearchParam()

    useEffect(() => {
        document.title = `${param('path')} - ${param('repo')} - Josh`
    });

    return (
        <div>
            <TopNav
                repo={param('repo')} 
                page="diff"
                filter={param('filter')} />

            <DiffViewer
                repo={param('repo')}
                path={param('path')}
                filter={param('filter')}
                rev={param('rev')}
                navigateCallback={useNavigateCallback()}
            />
        </div>
    )
}

function Sidebar() {
    const [params, _] = useSearchParams()

    return <div className="sidebar">
      <Link to={`/select?${createSearchParams(params)}`}>
        <img src={process.env.PUBLIC_URL.concat("/logo.png")} alt="Josh logo"/>
      </Link>
      <Link to={`/history?${createSearchParams(params)}`}>
        <GitCommitOutline
          color={'#aaaaaa'} 
          title="history"
          height="32px"
          width="32px"
        />
      </Link>
      <Link to={`/changes?${createSearchParams(params)}`}>
      <GitPullRequestOutline
        color={'#aaaaaa'} 
        title="changes"
        height="32px"
        width="32px"
      />
      </Link>
      <SearchOutline
        color={'#555555'} 
        title="search"
        height="32px"
        width="32px"
      />
    </div>;
}


function App() {
    return (
        <div>
        <BrowserRouter basename={'/~/ui'}>
            <Sidebar/>
            <Routes>
                <Route index element={<Navigate to="/select" />} />
                <Route path='/select' element={<Select />} />
                <Route path='/browse' element={<Browse />} />
                <Route path='/history' element={<History />} />
                <Route path='/changes' element={<Changes />} />
                <Route path='/view' element={<View />} />
                <Route path='/diff' element={<DiffView />} />
                <Route path='/change' element={<ChangeView />} />
            </Routes>
        </BrowserRouter>
        </div>
    );
}

export default App;
