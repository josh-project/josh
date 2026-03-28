import React from "react";
import {NavigateCallback, NavigateTargetType} from "./Navigation";

type BreadcrumbsProps = {
    repo: string
    path: string
    filter: string
    rev: string
    navigateCallback: NavigateCallback
}

export class Breadcrumbs extends React.Component<BreadcrumbsProps, {}> {
    renderEntries() {
        const navigateToEntry = (path: string) => {
            this.props.navigateCallback(NavigateTargetType.Directory, {
                repo:   this.props.repo,
                path:   path,
                filter: this.props.filter,
                rev:    this.props.rev
            })
        }

        const makeEntry = (label: string, path: string) => {
            const navigateCall = navigateToEntry.bind(this, path)
            return <span key={`bc-entry-${label}`} className={'breadcrumbs-entry'} onClick={navigateCall}>
                {label}
            </span>
        }

        const makeSpan = (index: number) => index !== 0 ?
            <span key={`bc-separator-${index}`} className={'breadcrumbs-separator'}>/</span> :
            ''

        const entries = this.props.path.split('/')
        const partialPaths = entries.map((_, i: number) => entries.slice(0, i + 1).join('/'))

        return Array<number>(entries.length * 2)
            .fill(0)
            .map((_, i) => {
                const j = Math.floor(i / 2)
                return (i % 2 === 0) ?
                    makeSpan(i) :
                    makeEntry(entries[j], partialPaths[j])
            })
    }

    render() {
        const navigateToRoot = () => {
            this.props.navigateCallback(NavigateTargetType.Directory, {
                repo:   this.props.repo,
                path:   '',
                filter: this.props.filter,
                rev:    this.props.rev
            })
        }

        return <nav className={'breadcrumbs'}>
            <span className={'breadcrumbs-entry breadcrumbs-entry-root'} onClick={navigateToRoot}>
                $ /
            </span>
            {this.props.path !== '' ? this.renderEntries() : ''}
        </nav>
    }
}
