import { gql } from 'graphql-request'

export enum NavigateTargetType {
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
query PathQuery($rev: String!, $filter: String!, $path: String!, $meta: String!) {
  rev(at:$rev, filter:$filter) {
    warnings {
      message
    }
    file(path:$path) {
      text
      meta(topic: $meta) {
        data {
          position: int(at: "/L")
          text: string(at: "/text")
        }
      }
    }
    dirs(at:$path,depth: 1) { path, meta(topic:$meta) { count } }
    files(at:$path,depth: 1) { 
      path, 
      meta(topic:$meta) { count } 
    }
  }
}
`
