#!/usr/bin/env bun
import { access, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { spawn } from "node:child_process";
import type { SpawnOptions } from "node:child_process";

type RunOptions = {
  cwd?: string;
  captureOutput?: boolean;
  allowNonZeroExit?: boolean;
  quiet?: boolean;
};

type RunResult = {
  code: number;
  stdout: string;
  stderr: string;
};

async function run(
  command: string,
  args: string[],
  options: RunOptions = {},
): Promise<RunResult> {
  const {
    cwd,
    captureOutput = false,
    allowNonZeroExit = false,
    quiet = false,
  } = options;

  if (!quiet) {
    const location = cwd ?? process.cwd();
    console.log(`[${location}] $ ${command} ${args.join(" ")}`);
  }

  const spawnOptions: SpawnOptions = {
    cwd,
    env: process.env,
    stdio: captureOutput ? ["ignore", "pipe", "pipe"] : "inherit",
  };

  return await new Promise((resolve, reject) => {
    const child = spawn(command, args, spawnOptions);

    let stdout = "";
    let stderr = "";

    if (captureOutput) {
      child.stdout?.setEncoding("utf8");
      child.stderr?.setEncoding("utf8");
      child.stdout?.on("data", (chunk: string) => {
        stdout += chunk;
      });
      child.stderr?.on("data", (chunk: string) => {
        stderr += chunk;
      });
    }

    child.on("error", reject);
    child.on("close", (code) => {
      const exitCode = code ?? 0;
      if (exitCode !== 0 && !allowNonZeroExit) {
        reject(
          new Error(
            `${command} ${args.join(
              " ",
            )} failed with code ${exitCode}${stderr ? `\n${stderr}` : ""}`,
          ),
        );
        return;
      }

      resolve({
        code: exitCode,
        stdout: stdout.trim(),
        stderr: stderr.trim(),
      });
    });
  });
}

async function ensurePathExists(targetPath: string, label: string) {
  try {
    await access(targetPath);
  } catch {
    throw new Error(`Cannot find ${label} at ${targetPath}`);
  }
}

async function ensureCleanWorkingTree(repoPath: string, label: string) {
  const status = await run("git", ["status", "--porcelain"], {
    cwd: repoPath,
    captureOutput: true,
    quiet: true,
  });

  if (status.stdout.trim().length > 0) {
    throw new Error(
      `${label} has uncommitted changes. Please stash or commit them before running the release script.`,
    );
  }
}

async function ensureRemoteExists(repoPath: string, remote: string) {
  const remotes = await run("git", ["remote"], {
    cwd: repoPath,
    captureOutput: true,
    quiet: true,
  });
  const hasRemote = remotes.stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .includes(remote);

  if (!hasRemote) {
    throw new Error(
      `Remote "${remote}" was not found in ${repoPath}. Please add it before running the script.`,
    );
  }
}

async function updateExtensionToml(filePath: string, version: string) {
  const raw = await readFile(filePath, "utf8");
  const versionMatch = raw.match(/^version\s*=\s*"([^"]+)"/m);
  if (!versionMatch) {
    throw new Error(
      `Could not find a version entry inside ${path.basename(filePath)}`,
    );
  }

  const currentVersion = versionMatch[1];
  if (currentVersion === version) {
    console.log(
      `extension.toml is already set to version ${version}, keeping existing value.`,
    );
    return currentVersion;
  }

  const updated = raw.replace(/^version\s*=\s*".*"/m, `version = "${version}"`);
  await writeFile(filePath, updated);
  console.log(
    `Updated extension.toml version from ${currentVersion} to ${version}.`,
  );
  return currentVersion;
}

async function updateZedExtensionsToml(filePath: string, version: string) {
  const raw = await readFile(filePath, "utf8");
  const lines = raw.split(/\r?\n/);
  let insideCodebook = false;

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line.startsWith("[") && line.endsWith("]")) {
      insideCodebook = line.trim() === "[codebook]";
      continue;
    }

    if (insideCodebook && line.trim().startsWith("version")) {
      const currentVersion = line.split("=")[1]?.trim().replace(/"/g, "");
      if (currentVersion === version) {
        console.log(
          `zed-extensions/extensions.toml already lists version ${version}.`,
        );
        return false;
      }

      lines[i] = `version = "${version}"`;
      await writeFile(filePath, lines.join("\n"));
      console.log(
        `Updated zed-extensions/extensions.toml entry for codebook to ${version}.`,
      );
      return true;
    }
  }

  throw new Error("Unable to find [codebook] entry in extensions.toml");
}

async function gitAddCommitTagAndPush(repoPath: string, version: string) {
  await run("git", ["add", "-A"], { cwd: repoPath });

  const diffResult = await run("git", ["diff", "--cached", "--quiet"], {
    cwd: repoPath,
    allowNonZeroExit: true,
    quiet: true,
  });

  if (diffResult.code === 0) {
    throw new Error("No changes staged in extension repo; aborting release.");
  }

  const commitMessage = `Codebook v${version}`;
  await run("git", ["commit", "-m", commitMessage], { cwd: repoPath });

  const tagName = `v${version}`;
  await run("git", ["tag", tagName], { cwd: repoPath });
  await run("git", ["push", "origin", "HEAD"], { cwd: repoPath });
  await run("git", ["push", "origin", tagName], { cwd: repoPath });
}

function normalizeVersion(input: string): string {
  const version = input.startsWith("v") ? input.slice(1) : input;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(
      `Invalid version "${input}". Expected a SemVer string like 0.2.4.`,
    );
  }
  return version;
}

async function main() {
  const repoRoot = process.cwd();
  const zedExtensionsPath = path.resolve(repoRoot, "..", "zed-extensions");

  const versionArg = process.argv[2];
  if (!versionArg) {
    console.error("Usage: bun scripts/release.ts <version>");
    process.exit(1);
  }

  const version = normalizeVersion(versionArg);
  console.log(`Starting release for Codebook v${version}`);

  const extensionTomlPath = path.join(repoRoot, "extension.toml");
  await ensurePathExists(extensionTomlPath, "extension.toml");

  await ensurePathExists(
    zedExtensionsPath,
    "the zed-extensions repository (../zed-extensions)",
  );

  await updateExtensionToml(extensionTomlPath, version);

  await gitAddCommitTagAndPush(repoRoot, version);

  console.log("\n---\nSwitching to zed-extensions workflow\n---");
  await ensureCleanWorkingTree(
    zedExtensionsPath,
    "The zed-extensions repository",
  );
  await ensureRemoteExists(zedExtensionsPath, "upstream");

  await run("git", ["checkout", "main"], { cwd: zedExtensionsPath });
  await run("git", ["fetch", "upstream"], { cwd: zedExtensionsPath });
  await run("git", ["pull", "upstream", "main"], { cwd: zedExtensionsPath });

  const branchName = `codebook-${version}`;
  const branchExists = await run("git", ["rev-parse", "--verify", branchName], {
    cwd: zedExtensionsPath,
    allowNonZeroExit: true,
    quiet: true,
  });
  if (branchExists.code === 0) {
    throw new Error(
      `Branch ${branchName} already exists in zed-extensions. Please remove or rename it before continuing.`,
    );
  }

  await run("git", ["checkout", "-b", branchName], {
    cwd: zedExtensionsPath,
  });

  await run(
    "git",
    ["submodule", "update", "--remote", "--merge", "extensions/codebook"],
    {
      cwd: zedExtensionsPath,
    },
  );

  const zedExtensionsTomlPath = path.join(zedExtensionsPath, "extensions.toml");
  await updateZedExtensionsToml(zedExtensionsTomlPath, version);

  await run("git", ["add", "extensions/codebook", "extensions.toml"], {
    cwd: zedExtensionsPath,
  });

  const zedDiff = await run("git", ["diff", "--cached", "--quiet"], {
    cwd: zedExtensionsPath,
    allowNonZeroExit: true,
    quiet: true,
  });
  if (zedDiff.code === 0) {
    throw new Error(
      "No staged changes in zed-extensions (expected submodule + extensions.toml updates).",
    );
  }

  const commitMessage = `Codebook v${version}`;
  await run("git", ["commit", "-m", commitMessage], { cwd: zedExtensionsPath });
  await run("git", ["push", "-u", "origin", branchName], {
    cwd: zedExtensionsPath,
  });

  console.log("\nRelease automation complete!");
  console.log(
    `Push created branch ${branchName} in zed-extensions. Create a PR for it when ready.`,
  );
}

main().catch((error) => {
  console.error(`\nRelease script failed: ${error.message}`);
  process.exit(1);
});
