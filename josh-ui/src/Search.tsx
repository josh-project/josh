import React from "react";
import {GraphQLClient} from 'graphql-request'
import {getServer} from "./Server";
import {NavigateCallback, NavigateTargetType, QUERY_CHANGES} from "./Navigation";
import {match} from "ts-pattern";

export type SearchBrowserProps = {
    repo: string
    filter: string
    searchstr: string
    rev: string
    navigateCallback: NavigateCallback
}

type State = {
    results: SearchResult[]
    client: GraphQLClient
}

export class SearchResults extends React.Component<SearchBrowserProps, State> {
    state: State = {
        results: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
    };

    startRequest() {
        this.state.client.rawRequest(QUERY_SEARCH, {
            searchstr: this.props.searchstr
            filter: this.props.filter,
            rev: this.props.rev
        }).then((d) => {
            const data = d.data

            this.setState({
                results: data.results
            })
        })
    }

    componentDidMount() {
        this.startRequest()
    }

    componentDidUpdate(
        prevProps: Readonly<ChangesBrowserProps>,
        prevState: Readonly<State>,
        snapshot?: any)
    {
        if (prevProps !== this.props) {
            this.setState({
                results: [],
            })

            this.startRequest()
        }
    }

    componentWillUnmount() {
        // TODO cancel request?
    }

    renderList(values: SearchResult[]) {

        const navigateBrowse = (rev: string, path: string, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.File, {
                repo:   this.props.repo,
                path:   path,
                filter: this.props.filter,
                rev:    rev,
            })
        }

        return values.map((ee) => {
            let entry = ee.commit;
            return <div key={entry.hash} className="commit-list-entry">
            <div className="changes-list-entry" >
                <span
                    className="change-summary"
                    onClick={navigateChange.bind(this, entry.original.hash)}>
                    {entry.summary}
                </span>
                <span
                    className="name"
                    onClick={navigateChange.bind(this, entry.original.hash)}>
                    {ee.name.replace("refs/heads/","")}
                </span>
                <span className="change-hash"
                    onClick={navigateBrowse.bind(this, entry.original.hash)}>

                {entry.hash.slice(0,6)}</span>
            </div>
            </div>

        })
    }

    render() {
        if (this.state.refs.length === 0) {
            return <div className={'history-browser-loading'}>Loading...</div>
        } else {
            return <div className={'changes-browser-list'}>
                {this.renderList(this.state.refs)}
            </div>
        }
    }
}
