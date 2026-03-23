# AGENTS.md

## Project Overview
This project is a harness analysis engine for Claude Code sessions. It analyzes session data and recommends harness improvements.

## Directory Purposes
- src/db/: Database logic
- src/analysis/: Session and data analysis
- src/commands/: CLI and command handling
- src/display/: Output formatting and display

## Agent Rules
- Require explicit file paths for all file operations; if missing, ask the user for clarification before proceeding.
- For each directory (e.g., src/analysis/, src/capture/), include a rule describing typical tasks and expected outputs.
- For any code change, propose a test run or verification step before marking the task complete.
- Only commit changes after successful verification or explicit user approval.
- Summarize actions, next steps, and any uncertainties after each major turn.
- If a prompt is vague or covers multiple goals, ask the user to break it into smaller, concrete steps.

## Prompting Guidelines
- Encourage users to specify file paths and concrete actions in every prompt.
- If a prompt is vague, ask clarifying questions before proceeding.

## Completion Rules
- When a task is finished, propose a test run to verify changes.
- If tests pass or the user approves, commit the changes with a clear message.

- When possible, request that users specify negative constraints (e.g., 'Do not modify src/db/') and provide reference implementations for new features.
