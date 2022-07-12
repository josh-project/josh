import React from "react";
import {GraphQLClient} from 'graphql-request'
import {getServer} from "./Server";
import {NavigateCallback, NavigateTargetType, QUERY_HISTORY} from "./Navigation";
import {match} from "ts-pattern";

export type HistoryBrowserProps = {
    repo: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

type Commit = {
    summary: string
    hash: string
}


type State = {
    commits: Commit[]
    client: GraphQLClient
}

export class HistoryList extends React.Component<HistoryBrowserProps, State> {
    state: State = {
        commits: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
    };

    startRequest() {
        this.state.client.rawRequest(QUERY_HISTORY, {
            rev: this.props.rev,
            filter: this.props.filter,
        }).then((d) => {
            const data = d.data.rev
            console.log("response", data);

            this.setState({
                commits: data.history
            })
        })
    }

    componentDidMount() {
        this.startRequest()
    }

    componentDidUpdate(prevProps: Readonly<HistoryBrowserProps>, prevState: Readonly<State>, snapshot?: any) {
        if (prevProps !== this.props) {
            this.setState({
                commits: [],
            })

            this.startRequest()
        }
    }

    componentWillUnmount() {
        // TODO cancel request?
    }

    renderList(values: Commit[], target: NavigateTargetType) {
        const classNameSuffix = match(target)
            .with(NavigateTargetType.Directory, () => 'dir')
            .run()

        const navigate = (rev: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(target, {
                repo:   this.props.repo,
                path:   '',
                filter: this.props.filter,
                rev:    rev,
            })
        }

        const formatCommit = (commit: Commit) => {
            return commit.hash.slice(0,8) + " " + commit.summary
        }

        return values.map((entry) => {
            const className = `commit-list-entry commit-list-entry-${classNameSuffix}`
            return <div className={className} key={entry.hash} onClick={navigate.bind(this, entry.hash)}>
                {formatCommit(entry)}
            </div>
        })
    }

    render() {
        console.log("render", this.state.commits);
        if (this.state.commits.length === 0) {
            return <div className={'history-browser-loading'}>Loading...</div>
        } else {
            return <div className={'history-browser-list'}>
                {this.renderList(this.state.commits, NavigateTargetType.Directory)}
            </div>
        }
    }
}
