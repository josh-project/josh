import './App.scss';
import {FileList} from './FileBrowser';

import {useEffect} from 'react';
import {None, Option} from 'tsoption';

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
import {HistoryList} from "./History";
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
            .with(NavigateTargetType.File, () => '/view')
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

function TopNav(props: { repo: string, filter: string }) {
    const selectParams = {
        repo: props.repo,
        filter: props.filter,
    }

    return <div className={'now-browsing'}>
        <span className={'now-browsing-name'}>
            <span className={'now-browsing-name-repo'}>
                now browsing: {props.repo}
            </span>
            {props.filter !== DEFAULT_FILTER && <span className={'now-browsing-name-filter'}>
                {props.filter}
            </span>}
        </span>
        <span className={'now-browsing-select'}>
            <Link to={`/select?${createSearchParams(selectParams)}`}>select repo</Link>
        </span>
    </div>
}

function Browse() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`/${param('path')} - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
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

function History() {
    const param = useStrictGetSearchParam()

    useTitleEffect(`History - ${param('repo')} - Josh`)

    return <div>
        <TopNav
            repo={param('repo')} 
            filter={param('filter')} />

        <HistoryList
            repo={param('repo')}
            filter={param('filter')}
            rev={param('rev')}
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

function App() {
    return (
        <BrowserRouter basename={'/~/ui'}>
            <Routes>
                <Route index element={<Navigate to="/select" />} />
                <Route path='/select' element={<Select />} />
                <Route path='/browse' element={<Browse />} />
                <Route path='/history' element={<History />} />
                <Route path='/view' element={<View />} />
            </Routes>
        </BrowserRouter>
    );
}

export default App;
