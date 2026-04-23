![仅保留单一历史](/splash.png)

借助对 Git 历史进行快速、增量且可逆的过滤，将 monorepo 的优点与 multirepo 的优势结合起来。

`josh-proxy` 可与任意 Git 托管服务集成：

```
$ docker run \
    -p 8000:8000 \
    -e JOSH_REMOTE=https://github.com \
    -v josh-vol:/data/git \
    joshproject/josh-proxy:latest
```

参见[Container options](https://josh-project.github.io/josh/reference/container.html) 页面，获取完整环境变量列表。

## 用例

### 部分克隆 <a href="https://josh-project.dev/~/ui/browse?repo=josh.git&path=&filter=%3A%2Fdocs&rev=HEAD"><img src="https://img.shields.io/badge/try_it-josh--project.dev-black"/></a>

通过将 monorepo 的子目录作为独立仓库来缩小克隆的范围与体积。

```
$ git clone https://josh-project.dev/josh.git:/docs.git
```

克隆的部分仓库（partial repo）在行为上像普通的 Git 仓库，但仅包含子目录下的文件，以及只包含影响这些文件的提交记录。该仓库既支持 fetch，也支持 push。

这不仅能减少工作树中文件的数量、提升客户端性能，还能利用 Git 的分布式开发能力与第三方在 monorepo 的某部分上协作。例如，可以只将仓库的选定部分镜像到公开的 GitHub 仓库或特定客户处。

### 项目组合/工作区（Workspace）

简化代码共享和依赖管理。除了对子目录的简单映射外，Josh 还支持对 monorepo 中的内容进行过滤、重映射和任意虚拟仓库的组合。

映射信息本身也存储在仓库中，同代码一起进行版本化管理。

<table>
    <thead>
        <tr>
            <th>中央 monorepo</th>
            <th>项目工作区</th>
            <th>workspace.josh 文件</th>
        </tr>
    </thead>
    <tbody>
        <tr>
            <td rowspan=2><img src="docs/src/img/central.svg?sanitize=true" alt="central.git 中的文件和文件夹" /></td>
            <td><img src="docs/src/img/project1.svg?sanitize=true" alt="project1.git 中的文件和文件夹" /></td>
            <td>
<pre>
dependencies = :/modules:[
    ::tools/
    ::library1/
]
</pre>
        </tr>
        <tr>
            <td><img src="docs/src/img/project2.svg?sanitize=true" alt="project2.git 中的文件和文件夹" /></td>
            <td>
<pre>libs/library1 = :/modules/library1</pre></td>
        </tr>
    </tbody>
</table>

工作区表现为普通的 Git 仓库：

```
$ git clone http://josh/central.git:workspace=workspaces/project1.git
```

### 简化的 CI/CD

将所有内容保存在单一仓库后，对每个交付物，CI/CD 系统只需查看一个源即可。而在传统的 monorepo 环境中，依赖管理由构建系统负责，构建系统通常针对特定语言定制，并且需要在文件系统上检出输入文件。因此在不克隆整个仓库并理解所用语言如何处理依赖的情况下，通常无法回答

> “某次提交会影响哪些交付物，需要重新构建哪些内容？”

这个问题。

尤其对于 C 系列语言，隐藏的头文件依赖很容易被忽略。因此通过沙箱限制编译器可见文件，以确保可重现构建，几乎是必要的。

使用 Josh，每个交付物都有自己的虚拟 Git 仓库，在 `workspace.josh` 文件中声明依赖。这样，要回答上述问题，就可以简单地比较提交 ID（commit IDs）。并且由于树过滤（tree filtering），每次构建都被严格沙箱化，仅能看到 monorepo 中实际被映射的部分。

因此，通常无需像常规构建工具那样克隆多个仓库，即可确定需要重新构建的交付物。

### GraphQL API

在不克隆仓库的情况下访问 Git 中的内容通常很有用——例如供 CI/CD 系统或 Web 前端（如 dashboard）使用。

Josh 为此提供了 GraphQL API。例如，可以用它查找树中当前存在的所有工作区：

```
query {
  rev(at:"refs/heads/master", filter:"::**/workspace.josh") {
    files { path }
  }
}
```

### 缓存代理

即便不使用部分克隆或工作区等高级功能，`josh-proxy` 也可以作为缓存代理，减少站点间流量或 CI 对主 Git 托管的大量请求。

## 常见问题

详见[常见问题（FAQ）](https://josh-project.github.io/josh/faq_zh_CN.html)页面。
