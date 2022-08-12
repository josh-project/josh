import './App.scss';
import {FileList} from './FileBrowser';

import {useEffect} from 'react';

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

    return (key: string): string => {
        let value = searchParams.get(key)
        if (value === null) {
            throw new Error(`Search param ${key} was not provided`)
        }

        return value
    }
}

function Select() {
    useEffect(() => {
        document.title = `Select repo - Josh`
    });

    return <div className={'ui-modal-container'}>
        <div className={'ui-modal'}>
            <RepoSelector navigateCallback={useNavigateCallback()}/>
        </div>
    </div>
}

function TopNav(props: { repo: string, filter: string}) {
    return <div className={'now-browsing'}>
        <span className={'now-browsing-name'}>
            <span className={'now-browsing-name-repo'}>
                now browsing: {props.repo} 
            </span>
            {props.filter !== ':/' && <span className={'now-browsing-name-filter'}>
                {props.filter}
            </span>}
        </span>
        <span className={'now-browsing-select'}>
            <Link to='/select'>select repo</Link>
        </span>
    </div>
}

function Browse() {
    const param = useGetSearchParam()

    useEffect(() => {
        document.title = `/${param('path')} - ${param('repo')} - Josh`
    });

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
    const param = useGetSearchParam()

    useEffect(() => {
        document.title = `History - ${param('repo')} - Josh`
    });

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
    const param = useGetSearchParam()

    useEffect(() => {
        document.title = `${param('path')} - ${param('repo')} - Josh`
    });

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
