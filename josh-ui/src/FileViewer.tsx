import React from "react";
import Editor from "@monaco-editor/react";
import {NavigateCallback, QUERY_PATH} from "./Navigation";
import {GraphQLClient} from "graphql-request";
import {getServer} from "./Server";
import {match} from "ts-pattern";

export type FileViewerProps = {
    repo: string
    path: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

type State = {
    content?: string
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

export class FileViewer extends React.Component<FileViewerProps, State> {
    state = {
        content: undefined,
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors'
        }),
    }

    componentDidMount() {
        this.state.client.rawRequest(QUERY_PATH, {
            rev: this.props.rev,
            filter: this.props.filter,
            path: this.props.path,
            meta: '',
        }).catch((reason) => {
            const data = reason.response.data.rev

            this.setState({
                content: data.file.text
            })
        })
    }

    render() {
        if (this.state.content !== undefined) {
            return <Editor
                value={this.state.content}
                language={mapLanguage(this.props.path)}
                height='80vh'
                theme='vs-dark'
                options={{
                    readOnly: true,
                    domReadOnly: true,
                    cursorBlinking: 'solid',
                }}
            />
        } else {
            return <div>Loading...</div>
        }
    }
}
