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
    authorEmail: string
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
            limit: 10,
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

        const navigateBrowse = (rev: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.Directory, {
                repo:   this.props.repo,
                path:   '',
                filter: this.props.filter,
                rev:    rev,
            })
        }

        const navigateChange = (rev: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.Change, {
                repo:   this.props.repo,
                path:   '',
                filter: this.props.filter,
                rev:    rev,
            })
        }


        return values.map((entry) => {
            return <div key={entry.hash} className="commit-list-entry">
            <div
                className="commit-list-entry-dir"
                onClick={navigateChange.bind(this, entry.original.hash)}>
                <span className="summary">{entry.summary}</span>
                <span className="authorEmail">{entry.authorEmail}</span>
            </div>
            <div
                className="commit-list-entry-browse"
                onClick={navigateBrowse.bind(this, entry.original.hash)}>
                Browse
            </div>
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
