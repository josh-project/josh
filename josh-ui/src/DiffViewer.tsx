import React from "react";
import {DiffEditor} from "@monaco-editor/react";
import {
    NavigateCallback,
    from_or_to_path,
    NavigateTargetType,
    QUERY_FILE_DIFF,
    ChangedFile
} from "./Navigation";
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
    summary: string
    all_files: ChangedFile[]
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
        summary: "",
        all_files: [],
        client: new GraphQLClient(`${getServer()}/~/graphql/${this.props.repo}`, {
            mode: 'cors',
            errorPolicy: 'all'
        }),
    }

    componentDidMount() {
        this.startRequest()
    }

    startRequest() {
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
                content_b: content_b,
                summary: data.summary,
                all_files: data.changedFiles,
            })
        })
    }

    componentDidUpdate(prevProps: Readonly<DiffViewerProps>, prevState: Readonly<State>, snapshot?: any) {
        if (prevProps !== this.props) {
            this.startRequest()
        }
    }

    render() {
        const navigate = (e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.Change, {
                repo:   this.props.repo,
                filter: this.props.filter,
                path:   '',
                rev:    this.props.rev
            })
        }

        let index = this.state.all_files.findIndex(f => from_or_to_path(f) === this.props.path);
        let prevclass = (index === 0) ?  "inactive" : "active";
        let nextclass = (index + 1 === this.state.all_files.length) ? "inactive" : "active";

        const navigate_diff = (delta: number, e: React.MouseEvent<HTMLDivElement>) => {
            this.props.navigateCallback(NavigateTargetType.Diff, {
                repo:   this.props.repo,
                filter: this.props.filter,
                path:   from_or_to_path(this.state.all_files[index+delta]),
                rev:    this.props.rev
            })
        }

        if (this.state.content_a !== undefined 
        &&  this.state.content_b !== undefined) {
            return <div>
                <div className="commit-message link" onClick={navigate.bind(this)}>{this.state.summary}</div>
                <div className="diff-view-filename">{this.props.path}
                <span className="prevnext">
                    <span className={prevclass} onClick={navigate_diff.bind(this, -1)}>&lt;</span>
                    <span className={nextclass} onClick={navigate_diff.bind(this, 1)}>&gt;</span>
                </span>
                </div>
                <DiffEditor
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
            </div>
        } else
        {
            return <div>Loading...</div>
        }
    }
}
