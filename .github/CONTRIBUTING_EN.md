# Contribution Guide

[简体中文](./CONTRIBUTING.md) | [English](./CONTRIBUTING_EN.md)

Thank you very much for your interest and contributions to BongoCat! Before submitting a contribution, please take a moment to read the following guidelines to ensure a smooth contribution process.

## Transparent Development

All work is conducted publicly on GitHub. Pull Requests from both core team members and external contributors follow the exact same code review process.

## Submitting Issues

We use [GitHub Issues](https://github.com/ayangweb/BongoCat/issues) for bug reports and new feature suggestions. Before submitting an issue, please ensure you have searched for existing similar issues, as they may have already been answered or are currently being addressed. For bug reports, please include full steps to reproduce the issue. For feature requests, please specify the changes you would like to see and the expected behavior.

## Submitting Pull Requests

### Collaboration Workflow

- **Claim an issue**: Create or claim an existing Issue on GitHub to let everyone know you are working on it, avoiding duplicated effort.
- **Development**: Complete your setup and work on bug fixes or feature development.
- **Submit PR**: Open a Pull Request for review.

### Prerequisites

- [Rust](https://v2.tauri.app/start/prerequisites/): Please follow the official guide to install the Rust environment.
- [Node.js](https://nodejs.org/en/): Required to run the project.
- [Pnpm](https://pnpm.io/): This project uses pnpm for package management.

### Install Dependencies

```shell
pnpm install
```

### Start Application

```shell
pnpm tauri dev
```

### Build Application

> If you need to debug after building, add `--debug` to the command below:

```shell
pnpm tauri build
```

## Commit Guidelines

Commit messages must follow the [conventional-changelog specification](https://www.conventionalcommits.org/en/v1.0.0/).

### Commit Types

Here is the list of supported commit types:

- `feat`: A new feature
- `fix`: A bug fix
- `docs`: Documentation updates
- `style`: Code style updates
- `refactor`: Code refactoring without adding new features or fixing bugs
- `perf`: Performance improvements
- `chore`: Other maintenance tasks

We look forward to your contributions—let's make BongoCat better together!
