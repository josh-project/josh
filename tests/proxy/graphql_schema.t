  $ . ${TESTDIR}/setup_test_env.sh
  $ cd ${TESTTMP}

  $ graphql-client introspect-schema http://localhost:8002/~/graphql/real_repo.git 2>/dev/null
  {
    "data": {
      "__schema": {
        "directives": [
          {
            "args": [
              {
                "defaultValue": null,
                "description": null,
                "name": "if",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              }
            ],
            "description": null,
            "locations": [
              "FIELD",
              "FRAGMENT_SPREAD",
              "INLINE_FRAGMENT"
            ],
            "name": "include"
          },
          {
            "args": [
              {
                "defaultValue": null,
                "description": null,
                "name": "if",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              }
            ],
            "description": null,
            "locations": [
              "FIELD",
              "FRAGMENT_SPREAD",
              "INLINE_FRAGMENT"
            ],
            "name": "skip"
          }
        ],
        "mutationType": {
          "name": "RepositoryMut"
        },
        "queryType": {
          "name": "Repository"
        },
        "subscriptionType": null,
        "types": [
          {
            "description": null,
            "enumValues": null,
            "fields": null,
            "inputFields": null,
            "interfaces": null,
            "kind": "SCALAR",
            "name": "Boolean",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "commit",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "topic",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "add",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "LIST",
                        "name": null,
                        "ofType": {
                          "kind": "NON_NULL",
                          "name": null,
                          "ofType": {
                            "kind": "INPUT_OBJECT",
                            "name": "MarkersInput",
                            "ofType": null
                          }
                        }
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "meta",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "RepositoryMut",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "description",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "type",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "__Type",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "defaultValue",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__InputValue",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "filter",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "hash",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "summary",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "format",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "date",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "filter",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "original",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Boolean",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "rev",
                "type": {
                  "kind": "OBJECT",
                  "name": "Revision",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "parents",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "Revision",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "depth",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Int",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "files",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "Path",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "depth",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Int",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "dirs",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "Path",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "path",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "file",
                "type": {
                  "kind": "OBJECT",
                  "name": "Path",
                  "ofType": null
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "path",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "dir",
                "type": {
                  "kind": "OBJECT",
                  "name": "Path",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "warnings",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "Warning",
                      "ofType": null
                    }
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Revision",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": null,
            "inputFields": null,
            "interfaces": null,
            "kind": "SCALAR",
            "name": "String",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "description",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "args",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "__InputValue",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "type",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "__Type",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "isDeprecated",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "deprecationReason",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__Field",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "data",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "Document",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "count",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Int",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Markers",
            "possibleTypes": null
          },
          {
            "description": "GraphQL type kind\n\nThe GraphQL specification defines a number of type kinds - the meta type of a type.",
            "enumValues": [
              {
                "deprecationReason": null,
                "description": "## Scalar types\n\nScalar types appear as the leaf nodes of GraphQL queries. Strings, numbers, and booleans are the built in types, and while it's possible to define your own, it's relatively uncommon.",
                "isDeprecated": false,
                "name": "SCALAR"
              },
              {
                "deprecationReason": null,
                "description": "## Object types\n\nThe most common type to be implemented by users. Objects have fields and can implement interfaces.",
                "isDeprecated": false,
                "name": "OBJECT"
              },
              {
                "deprecationReason": null,
                "description": "## Interface types\n\nInterface types are used to represent overlapping fields between multiple types, and can be queried for their concrete type.",
                "isDeprecated": false,
                "name": "INTERFACE"
              },
              {
                "deprecationReason": null,
                "description": "## Union types\n\nUnions are similar to interfaces but can not contain any fields on their own.",
                "isDeprecated": false,
                "name": "UNION"
              },
              {
                "deprecationReason": null,
                "description": "## Enum types\n\nLike scalars, enum types appear as the leaf nodes of GraphQL queries.",
                "isDeprecated": false,
                "name": "ENUM"
              },
              {
                "deprecationReason": null,
                "description": "## Input objects\n\nRepresents complex values provided in queries _into_ the system.",
                "isDeprecated": false,
                "name": "INPUT_OBJECT"
              },
              {
                "deprecationReason": null,
                "description": "## List types\n\nRepresent lists of other types. This library provides implementations for vectors and slices, but other Rust types can be extended to serve as GraphQL lists.",
                "isDeprecated": false,
                "name": "LIST"
              },
              {
                "deprecationReason": null,
                "description": "## Non-null types\n\nIn GraphQL, nullable types are the default. By putting a `!` after a type, it becomes non-nullable.",
                "isDeprecated": false,
                "name": "NON_NULL"
              }
            ],
            "fields": null,
            "inputFields": null,
            "interfaces": null,
            "kind": "ENUM",
            "name": "__TypeKind",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "path",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "relative",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "dir",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Path",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "topic",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "meta",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Markers",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "filter",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "rev",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Revision",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "hash",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "text",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "toml",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Document",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "json",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Document",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "yaml",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Document",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Path",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "description",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "kind",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "ENUM",
                    "name": "__TypeKind",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": "false",
                    "description": null,
                    "name": "includeDeprecated",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Boolean",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "fields",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "__Field",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "ofType",
                "type": {
                  "kind": "OBJECT",
                  "name": "__Type",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "inputFields",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "__InputValue",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "interfaces",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "__Type",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "possibleTypes",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "__Type",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": "false",
                    "description": null,
                    "name": "includeDeprecated",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Boolean",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "enumValues",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "__EnumValue",
                      "ofType": null
                    }
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__Type",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": null,
            "inputFields": [
              {
                "defaultValue": null,
                "description": null,
                "name": "path",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "defaultValue": null,
                "description": null,
                "name": "data",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                }
              }
            ],
            "interfaces": null,
            "kind": "INPUT_OBJECT",
            "name": "MarkersInput",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "message",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Warning",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "default",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "string",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "default",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Boolean",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "bool",
                "type": {
                  "kind": "SCALAR",
                  "name": "Boolean",
                  "ofType": null
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "default",
                    "type": {
                      "kind": "SCALAR",
                      "name": "Int",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "int",
                "type": {
                  "kind": "SCALAR",
                  "name": "Int",
                  "ofType": null
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "list",
                "type": {
                  "kind": "LIST",
                  "name": null,
                  "ofType": {
                    "kind": "NON_NULL",
                    "name": null,
                    "ofType": {
                      "kind": "OBJECT",
                      "name": "Document",
                      "ofType": null
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "value",
                "type": {
                  "kind": "OBJECT",
                  "name": "Document",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "id",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Document",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "types",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "__Type",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "queryType",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "__Type",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "mutationType",
                "type": {
                  "kind": "OBJECT",
                  "name": "__Type",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "subscriptionType",
                "type": {
                  "kind": "OBJECT",
                  "name": "__Type",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "directives",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "__Directive",
                        "ofType": null
                      }
                    }
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__Schema",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": null,
            "inputFields": null,
            "interfaces": null,
            "kind": "SCALAR",
            "name": "Int",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "description",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "isDeprecated",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "deprecationReason",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__EnumValue",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": [
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "QUERY"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "MUTATION"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "SUBSCRIPTION"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "FIELD"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "FRAGMENT_DEFINITION"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "FRAGMENT_SPREAD"
              },
              {
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "INLINE_FRAGMENT"
              }
            ],
            "fields": null,
            "inputFields": null,
            "interfaces": null,
            "kind": "ENUM",
            "name": "__DirectiveLocation",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "description",
                "type": {
                  "kind": "SCALAR",
                  "name": "String",
                  "ofType": null
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "locations",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "ENUM",
                        "name": "__DirectiveLocation",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "args",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "__InputValue",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": "Use the locations array instead",
                "description": null,
                "isDeprecated": true,
                "name": "onOperation",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": "Use the locations array instead",
                "description": null,
                "isDeprecated": true,
                "name": "onFragment",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              },
              {
                "args": [],
                "deprecationReason": "Use the locations array instead",
                "description": null,
                "isDeprecated": true,
                "name": "onField",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "Boolean",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "__Directive",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "pattern",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "refs",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "LIST",
                    "name": null,
                    "ofType": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "OBJECT",
                        "name": "Reference",
                        "ofType": null
                      }
                    }
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "at",
                    "type": {
                      "kind": "NON_NULL",
                      "name": null,
                      "ofType": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": null
                      }
                    }
                  },
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "filter",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "rev",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Revision",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Repository",
            "possibleTypes": null
          },
          {
            "description": null,
            "enumValues": null,
            "fields": [
              {
                "args": [],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "name",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "SCALAR",
                    "name": "String",
                    "ofType": null
                  }
                }
              },
              {
                "args": [
                  {
                    "defaultValue": null,
                    "description": null,
                    "name": "filter",
                    "type": {
                      "kind": "SCALAR",
                      "name": "String",
                      "ofType": null
                    }
                  }
                ],
                "deprecationReason": null,
                "description": null,
                "isDeprecated": false,
                "name": "rev",
                "type": {
                  "kind": "NON_NULL",
                  "name": null,
                  "ofType": {
                    "kind": "OBJECT",
                    "name": "Revision",
                    "ofType": null
                  }
                }
              }
            ],
            "inputFields": null,
            "interfaces": [],
            "kind": "OBJECT",
            "name": "Reference",
            "possibleTypes": null
          }
        ]
      }
    }
  } (no-eol)

