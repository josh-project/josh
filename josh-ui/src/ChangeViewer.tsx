import React from "react";
import {GraphQLClient} from 'graphql-request'
import {getServer} from "./Server";
import {NavigateCallback, NavigateTargetType, QUERY_CHANGE} from "./Navigation";
import {match} from "ts-pattern";

export type ChangeViewProps = {
    repo: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

type Path = {
    path: string
}

type ChangedFile = {
    from: Path
    to: Path
}

type State = {
    summary: string
    files: ChangedFile[]
    client: GraphQLClient
}

export class ChangeViewer extends React.Component<ChangeViewProps, State> {
    state: State = {
        summary: "",
        files: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
    };

    startRequest() {
        this.state.client.rawRequest(QUERY_CHANGE, {
            rev: this.props.rev,
            filter: this.props.filter,
        }).then((d) => {
            const data = d.data.rev

            this.setState({
                summary: data.summary,
                files: data.changedFiles,
            })
        })
    }

    componentDidMount() {
        this.startRequest()
    }

    componentDidUpdate(prevProps: Readonly<ChangeViewProps>, prevState: Readonly<State>, snapshot?: any) {
        if (prevProps !== this.props) {
            this.setState({
                files: [],
            })

            this.startRequest()
        }
    }

    componentWillUnmount() {
        // TODO cancel request?
    }

    renderList(values: ChangedFile[], target: NavigateTargetType) {
        const classNameSuffix = match(target)
            .with(NavigateTargetType.Diff, () => 'file')
            .run()

        const navigate = (path: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(target, {
                repo:   this.props.repo,
                filter: this.props.filter,
                path:   path,
                rev:    this.props.rev
            })
        }

        return values.map((entry) => {
            const className = `file-browser-list-entry file-browser-list-entry-${classNameSuffix}`
            let path = "";
            let prefix = "M";
            if (!entry.from) {
                prefix = "A";
                path = entry.to.path;
            }
            else if (!entry.to) {
                prefix = "D";
                path = entry.from.path;
            }
            else {
                path = entry.from.path;
            }

            return <div className={className} key={path} onClick={navigate.bind(this,path)}>
                <span>{prefix}</span>{path}
            </div>
        })
    }

    render() {
        if (this.state.files.length === 0) {
            return <div className={'file-browser-loading'}>Loading...</div>
        } else {
            return <div>
                <div>
                    {this.state.summary}
                </div>
                <div className={'file-browser-list'}>
                    {this.renderList(this.state.files, NavigateTargetType.Diff)}
                </div>
            </div>
        }
    }
}
