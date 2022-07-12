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
    let trimSuffix = (repo: string) => {
        return repo.replace(/\.git$/, '')
    }

    return match(url)
        .with(when((v: string) => v.startsWith('git@')),
            () => ({ type: 'ProtocolNotSupported' } as UrlCheckResult))
        .with(when((v: string) => v.startsWith(expectedPath)),
            (v) => ({ type: 'RemoteFound', path: trimSuffix(v.replace(expectedPath, '')) } as UrlCheckResult))
        .with(when((v: string) => !(v.startsWith('http://') || v.startsWith('https://'))),
            (v) => ({ type: 'RemoteFound', path: trimSuffix(v) }) as UrlCheckResult)
        .otherwise(() => ({ type: 'RemoteMismatch' } as UrlCheckResult))
}

function formatHint(checkResult: UrlCheckResult, filter: Option<string>): string {
    const makeCheckoutHint = (repo: string): string => {
        const formattedFilter = filter.map(v => v +  '.git').getOrElse('')
        return `Checkout URL: ${getServer()}/${repo}.git${formattedFilter}`
    }

    return match(checkResult)
        .with({ type: 'ProtocolNotSupported' },
            () => 'Only HTTPS access is currently supported')
        .with({ type: 'NotAGitRepo' },
            () => 'Repository URL should end in .git')
        .with({ type: 'RemoteFound', path: select() },
            (path) => makeCheckoutHint(path))
        .otherwise(() => 'Repository is not located on the current remote')
}

type RepoSelectorProps = {
    navigateCallback: NavigateCallback
}

type State = {
    remote: Remote
    repo: Option<string>
    filter: Option<string>
}

type ParsedInput = {
    hint: Option<string>
    target: [Option<string>, Option<string>]
    label: boolean
}

export class RepoSelector extends React.Component<RepoSelectorProps, State> {
    state: State = {
        remote: { type: 'None' },
        repo: new None(),
        filter: new None(),
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

    parseUserInput = (): ParsedInput => {
        const parseWithRepo = (remote: string, repo: string): ParsedInput => {
            const expectedPath = remote + '/'
            const checkResult = checkUrl(repo, expectedPath)

            const maybeFilter = this.state.filter
            const maybeRepo = match(checkResult)
                .with({ type: 'RemoteFound', path: select() },
                    (path) => Option.of(path))
                .otherwise(() => new None<string>())

            const hint = formatHint(checkResult, maybeFilter)
            const label = !repo.startsWith(remote)

            return {
                hint: Option.of(hint),
                target: [maybeRepo, maybeFilter],
                label: label,
            }
        }

        const parseWithRemote = (remote: string): ParsedInput => {
            return this.state.repo.map((repo: string) => {
                return parseWithRepo(remote, repo)
            }).getOrElse({
                hint: new None(),
                target: [new None(), new None()],
                label: true,
            })
        }

        return match(this.state.remote)
            .with({ type: 'Some', value: select() }, (remote): ParsedInput => {
                return parseWithRemote(remote)
            })
            .with({ type: 'None' }, (): ParsedInput => {
                return {
                    hint: new None(),
                    target: [new None(), new None()],
                    label: true,
                }
            })
            .run()
    }

    buttonPressed = (e: React.MouseEvent<HTMLButtonElement>) => {
        const parsedInput = this.parseUserInput()

        if (parsedInput.target[0].isEmpty()) {
            return
        }

        this.props.navigateCallback(NavigateTargetType.History, {
            repo:   parsedInput.target[0].getOrElse('') + '.git',
            path:   '',
            filter: parsedInput.target[1].getOrElse(':/'),
            rev:    'HEAD',
        })
    }

    render() {
        const fieldChanged = (setCallable: (_: Option<string>) => void, e: React.ChangeEvent<HTMLInputElement>) => {
            const value = e.target.value === '' ? new None<string>() : Option.of(e.target.value)
            setCallable(value)
        }

        const repoChanged = fieldChanged.bind(this, (v) => this.setState({ repo: v }))
        const filterChanged = fieldChanged.bind(this, (v) => this.setState({ filter: v }))

        const parsedInput = this.parseUserInput()

        return <div>
            <h3>Select repo</h3>
            <div className={'repo-selector-repo'}>
                { parsedInput.label &&
                    <span className={'repo-selector-status-label'}>
                        {this.getStatus()}
                    </span>
                }
                <input
                    type={'text'}
                    className={'repo-selector-repo-input ui-input'}
                    placeholder={'repo.git'}
                    onChange={repoChanged}
                />
            </div>
            <div className={'repo-selector-filter'}>
                <input
                    type={'text'}
                    className={'repo-selector-filter-input ui-input'}
                    placeholder={':filter'}
                    onChange={filterChanged}
                />
            </div>
            { parsedInput.hint.nonEmpty() &&
                <div className={'repo-selector-hint'}>
                    {parsedInput.hint.getOrElse('')}
                </div>
            }
            <button onClick={this.buttonPressed} className={'ui-button repo-selector-button'}>
                Browse
            </button>
        </div>
    }
}
