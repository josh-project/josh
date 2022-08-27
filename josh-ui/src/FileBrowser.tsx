import React from "react";
import {GraphQLClient} from 'graphql-request'
import {getServer} from "./Server";
import {NavigateCallback, NavigateTargetType, QUERY_PATH} from "./Navigation";
import {match} from "ts-pattern";

export type FileBrowserProps = {
    repo: string
    path: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

type State = {
    dirs: string[]
    files: string[]
    client: GraphQLClient
    loading: boolean
    path: string,
}

type FileOrDir = {
    path: string
}

export class FileList extends React.Component<FileBrowserProps, State> {
    state: State = {
        dirs: [],
        files: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
        loading: true,
        path: this.props.path,
    };

    startRequest() {
        this.state.client.rawRequest(QUERY_PATH, {
            rev: this.props.rev,
            filter: this.props.filter,
            path: this.props.path,
        }).catch((reason) => {
            const data = reason.response.data.rev

            this.setState({
                dirs: data.dirs.map((v: FileOrDir) => v.path),
                files: data.files.map((v: FileOrDir) => v.path),
                path: this.props.path,
                loading: false,
            })
        })
    }

    componentDidMount() {
        this.startRequest()
    }

    componentDidUpdate(prevProps: Readonly<FileBrowserProps>, prevState: Readonly<State>, snapshot?: any) {
        if (prevProps !== this.props) {
            this.setState({
                loading: true,
            })

            this.startRequest()
        }
    }

    componentWillUnmount() {
        // TODO cancel request?
    }

    renderList(values: string[], target: NavigateTargetType) {
        const classNameSuffix = match(target)
            .with(NavigateTargetType.File, () => 'file')
            .with(NavigateTargetType.Directory, () => 'dir')
            .run()

        const navigate = (path: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(target, {
                repo:   this.props.repo,
                path:   path,
                filter: this.props.filter,
                rev:    this.props.rev
            })
        }

        const formatName = (path: string) => {
            const baseName = path.indexOf(this.state.path + '/') !== -1 ?
                path.slice(this.state.path.length + 1) :
                path

            return match(target)
                .with(NavigateTargetType.Directory, () => baseName + '/')
                .with(NavigateTargetType.File, () => baseName)
                .run()
        }

        return values.map((entry) => {
            const className = `file-browser-list-entry file-browser-list-entry-${classNameSuffix}`
            const onClick = this.state.loading ? undefined : navigate.bind(this, entry)
            return <div className={className} key={entry} onClick={onClick}>
                {formatName(entry)}
            </div>
        })
    }

    render() {
        if (this.state.dirs.length === 0 && this.state.files.length === 0) {
            return <div className={'file-browser-loading'}>Loading...</div>
        } else {
            const maybeLoadingClass = this.state.loading ? 'file-browser-list-loading' : ''
            return <div className={`file-browser-list ${maybeLoadingClass}`}>
                {this.renderList(this.state.dirs, NavigateTargetType.Directory)}
                {this.renderList(this.state.files, NavigateTargetType.File)}
            </div>
        }
    }
}
