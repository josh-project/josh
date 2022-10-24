import React from "react";
import {GraphQLClient} from 'graphql-request'
import {getServer} from "./Server";
import {NavigateCallback, NavigateTargetType, QUERY_CHANGES} from "./Navigation";
import {match} from "ts-pattern";

export type ChangesBrowserProps = {
    repo: string
    filter: string
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

type Ref = {
    name: string,
    commit: Commit,
}


type State = {
    refs: Ref[]
    client: GraphQLClient
}

export class ChangesList extends React.Component<ChangesBrowserProps, State> {
    state: State = {
        refs: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
    };

    startRequest() {
        this.state.client.rawRequest(QUERY_CHANGES, {
            filter: this.props.filter,
        }).then((d) => {
            const data = d.data

            this.setState({
                refs: data.refs
            })
        })
    }

    componentDidMount() {
        this.startRequest()
    }

    componentDidUpdate(prevProps: Readonly<ChangesBrowserProps>, prevState: Readonly<State>, snapshot?: any) {
        if (prevProps !== this.props) {
            this.setState({
                refs: [],
            })

            this.startRequest()
        }
    }

    componentWillUnmount() {
        // TODO cancel request?
    }

    renderList(values: Ref[]) {

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
