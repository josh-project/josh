import { gql } from 'graphql-request'

export enum NavigateTargetType {
  History,
  File,
  Directory,
  Change,
  Diff
}

export type NavigateTarget = {
  repo: string
  path: string
  filter: string
  rev: string
}

export type Path = {
    path: string
}

export type ChangedFile = {
    from: Path
    to: Path
}

export function from_or_to_path(f: ChangedFile) {
    if (!f.from) {
        return f.to.path;
    }
    return f.from.path;
}

export type NavigateCallback = (targetType: NavigateTargetType, target: NavigateTarget) => void

export const QUERY_DIR = gql`
query($rev: String!, $filter: String!, $path: String!) {
  rev(at:$rev, filter:$filter) {
    warnings {
      message
    }
    dirs(at:$path,depth: 1) { path }
    files(at:$path,depth: 1) { 
      path,
    }
  }
}
`

export const QUERY_FILE = gql`
query($rev: String!, $filter: String!, $path: String!) {
  rev(at:$rev, filter:$filter) {
    file(path:$path) {
      text
    }
  }
}
`

export const QUERY_FILE_DIFF = gql`
query($rev: String!, $filter: String!, $path: String!) {
  rev(at:$rev, filter:$filter) {
    summary
    history(limit: 2) {
      file(path:$path) {
        text
      }
    }
    changedFiles {
      from {
        path
      }
      to {
        path
      }
    }
  }
}
`

export const QUERY_HISTORY = gql`
query($rev: String!, $filter: String!, $limit: Int) {
  rev(at:$rev, filter:$filter) {
    history(limit: $limit) {
      summary
      authorEmail
      hash
      original: rev { hash }
    }
  }
}
`

export const QUERY_CHANGES = gql`
query($filter: String!) {
  refs(pattern:"refs/heads/@changes/*") {
    name
    commit: rev(filter: $filter) {
      summary
      authorEmail
      hash
      original: rev { hash }
    }
  }
}
`

export const QUERY_CHANGE = gql`
query($rev: String!, $filter: String!) {
  rev(at:$rev, filter:$filter) {
    summary: message
    authorEmail
    hash
    rev { hash }
    changedFiles {
      from {
        path
      }
      to {
        path
      }
    }
  }
}
`
