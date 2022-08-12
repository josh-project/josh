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

type Original = {
    hash: string
}

type Commit = {
    summary: string
    hash: string
    original: Original
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
            limit: 100,
        }).then((d) => {
            const data = d.data.rev

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

    renderList(values: Commit[]) {

        const navigate = (rev: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.Directory, {
                repo:   this.props.repo,
                path:   '',
                filter: this.props.filter,
                rev:    rev,
            })
        }

        return values.map((entry) => {
            const className = `commit-list-entry commit-list-entry-dir`
            return <div
                className={className}
                key={entry.hash}
                onClick={navigate.bind(this, entry.original.hash)}>
                <span className="hash">{entry.hash.slice(0,8)}</span>
                <span className="summary">{entry.summary}</span>
            </div>
        })
    }

    render() {
        if (this.state.commits.length === 0) {
            return <div className={'history-browser-loading'}>Loading...</div>
        } else {
            return <div className={'history-browser-list'}>
                {this.renderList(this.state.commits)}
            </div>
        }
    }
}
