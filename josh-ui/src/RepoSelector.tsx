import React from 'react';
import {match, select, when} from 'ts-pattern';
import {None, Option} from 'tsoption';
import {getServer} from './Server';
import {NavigateCallback, NavigateTargetType} from "./Navigation";

type Remote =
    | { type: 'None' }
    | { type: 'Some';  value: string }
    | { type: 'Error'; error: Error }

type UrlCheckResult =
    | { type: 'RemoteMismatch'; }
    | { type: 'ProtocolNotSupported'; }
    | { type: 'NotAGitRepo'; }
    | { type: 'RemoteFound'; path: string }

function checkUrl(url: string, expectedPath: string): UrlCheckResult {
    return match(url)
        .with(when((v: string) => v.startsWith('git@')),
            () => ({ type: 'ProtocolNotSupported' } as UrlCheckResult))
        .with(when((v: string) => !v.endsWith('.git')),
            () => ({ type: 'NotAGitRepo' } as UrlCheckResult))
        .with(when((v: string) => v.startsWith(expectedPath)),
            (v) => ({ type: 'RemoteFound', path: v.replace(expectedPath, '') } as UrlCheckResult))
        .with(when((v: string) => !(v.startsWith('http://') || v.startsWith('https://'))),
            (v) => ({ type: 'RemoteFound', path: v }) as UrlCheckResult)
        .otherwise(() => ({ type: 'RemoteMismatch' } as UrlCheckResult))
}

type RepoSelectorProps = {
    navigateCallback: NavigateCallback
}

type State = {
    remote: Remote
    hint: Option<string>
    repo: Option<string>
    input: string,
}

export class RepoSelector extends React.Component<RepoSelectorProps, State> {
    state: State = {
        remote: { type: 'None' },
        hint: new None(),
        repo: new None(),
        input: '',
    };

    componentDidMount () {
        fetch(getServer() + '/remote')
            .then(response => response.text())
            .then(response => this.setState({
                remote: { type: 'Some', value: response },
            }))
            .catch(error => this.setState({
                remote: { type: 'Error', error: new Error(error) },
            }))
    }

    getStatus = () => {
        return match(this.state.remote)
            .with({ type: 'None' }, () => 'loading...' )
            .with({ type: 'Error', error: select() }, (e) => `error: ${e.message}` )
            .with({ type: 'Some', value: select() }, (v) => `${v}/` )
            .exhaustive()
    }

    isLabelVisible = () => {
        return match(this.state.remote)
            .with({ type: 'Some', value: when((v) => this.state.input.startsWith(v)) }, () => false )
            .otherwise(() => true)
    }

    getHint = () => {
        return this.state.hint.isEmpty() ? false : <div className={'repo-selector-hint'}>
            {this.state.hint.getOrElse('')}
        </div>
    }

    formatHint = (v: string): string => {
        return `Checkout URL: ${getServer()}/${v}`
    }

    inputChanged = (e: React.ChangeEvent<HTMLInputElement>) => {
        const getHint = (expectedPath: string) => {
            const checkResult = checkUrl(e.target.value, expectedPath)
            const hint = match(checkResult)
                .with({ type: 'ProtocolNotSupported' },
                    () => Option.of('Only HTTPS access is currently supported'))
                .with({ type: 'NotAGitRepo' },
                    () => Option.of('Repository URL should end in .git'))
                .with({ type: 'RemoteFound', path: select() },
                    (path) => Option.of(this.formatHint(path)))
                .otherwise(() => Option.of('Repository is not located on the current remote'))

            const repo = match(checkResult)
                .with({ type: 'RemoteFound', path: select() },
                    (path) => Option.of(path))
                    .otherwise(() => new None<string>())

            return [hint, repo]
        }

        match(this.state.remote)
            .with({ type: 'Some', value: select() }, (remote) => {
                if (e.target.value === '') {
                    return
                }

                const expectedPath = remote + '/'
                const [hint, repo] = getHint(expectedPath)

                this.setState({
                    hint: hint,
                    repo: repo,
                    input: e.target.value,
                })
            })
            .with({ type: 'None' }, () => {
                this.setState({
                    repo: new None()
                })
            })
            .run()
    }

    buttonPressed = (e: React.MouseEvent<HTMLButtonElement>) => {
        if (this.state.repo.isEmpty()) {
            return
        }

        this.props.navigateCallback(NavigateTargetType.Directory, {
            repo:   this.state.repo.getOrElse(''),
            path:   '',
            filter: ':/',
            rev:    'HEAD',
        })
    }

    render() {
        return <div>
            <h3>Select repo</h3>
            <div className={'repo-selector-repo'}>
                { this.isLabelVisible() &&
                    <span className={'repo-selector-status-label'}>
                        {this.getStatus()}
                    </span>
                }
                <input
                    type={'text'}
                    className={'repo-selector-input ui-input'}
                    onChange={this.inputChanged}
                />
            </div>
            {this.getHint()}
            <button onClick={this.buttonPressed} className={'ui-button repo-selector-button'}>
                Browse
            </button>
        </div>
    }
}
