import { gql } from 'graphql-request'

export enum NavigateTargetType {
  History,
  File,
  Directory
}

export type NavigateTarget = {
  repo: string
  path: string
  filter: string
  rev: string
}

export type NavigateCallback = (targetType: NavigateTargetType, target: NavigateTarget) => void

export const QUERY_PATH = gql`
query PathQuery($rev: String!, $filter: String!, $path: String!) {
  rev(at:$rev, filter:$filter) {
    warnings {
      message
    }
    file(path:$path) {
      text
    }
    dirs(at:$path,depth: 1) { path }
    files(at:$path,depth: 1) { 
      path,
    }
  }
}
`
export const QUERY_HISTORY = gql`
query HistoryQuery($rev: String!, $filter: String!, $limit: Int) {
  rev(at:$rev, filter:$filter) {
    history(limit: $limit) {
      summary
      hash
      original: rev(original: true) { hash }
    }
  }
}
`
