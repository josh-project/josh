import React from "react";
import {DiffEditor} from "@monaco-editor/react";
import {NavigateCallback, QUERY_FILE_DIFF} from "./Navigation";
import {GraphQLClient} from "graphql-request";
import {getServer} from "./Server";
import {match} from "ts-pattern";

export type DiffViewerProps = {
    repo: string
    path: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

type State = {
    content_a?: string
    content_b?: string
    client: GraphQLClient
}

function mapLanguage(path: string) {
    const extension = path.split('.').pop()

    return match(extension)
        .with('css', () => 'css')
        .with('html', 'htm', 'xhtml', () => 'html')
        .with('json', () => 'json')
        .with('ts', 'ts.d', 'tsx', () => 'typescript')
        .with('md', () => 'markdown')
        .with('rs', () => 'rust')
        .with('Dockerfile', () => 'dockerfile')
        .otherwise(() => undefined)
}

export class DiffViewer extends React.Component<DiffViewerProps, State> {
    state = {
        content_a: undefined,
        content_b: undefined,
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors',
            errorPolicy: 'all'
        }),
    }

    componentDidMount() {
        this.state.client.rawRequest(QUERY_FILE_DIFF, {
            rev: this.props.rev,
            filter: this.props.filter,
            path: this.props.path,
        }).then((d) => {
            const data = d.data.rev

            let content_a = "";
            let content_b = "";

            if (data.history[1].file) {
                content_a = data.history[1].file.text
            }

            if (data.history[0].file) {
                content_b = data.history[0].file.text
            }

            this.setState({
                content_a: content_a,
                content_b: content_b
            })
        })
    }

    render() {
        if (this.state.content_a !== undefined 
        &&  this.state.content_b !== undefined) {
            return <DiffEditor
                modified={this.state.content_b}
                original={this.state.content_a}
                language={mapLanguage(this.props.path)}
                height='80vh'
                theme='vs-dark'
                options={{
                    readOnly: true,
                    domReadOnly: true,
                    cursorBlinking: 'solid',
                }}
            />
        } else
        {
            return <div>Loading...</div>
        }
    }
}
