import fs from "node:fs";
import nodePath from "node:path";

export function fsStats(folderPath: string) {
  const stats = fs.statfsSync(folderPath);
  const freeSpace = stats.bavail * stats.bsize;
  const totalSpace = stats.blocks * stats.bsize;
  return { freeSpace, totalSpace };
}

export function getFolderSize(folderPath: string): number {
  const files = fs.readdirSync(folderPath, { withFileTypes: true });

  const paths = files.map((file) => {
    const path = nodePath.join(folderPath, file.name);
    if (file.isDirectory()) {
      return getFolderSize(path);
    }
    if (file.isFile()) {
      return fs.statSync(path).size;
    }
    return 0;
  });

  return paths
    .flat(Infinity)
    .reduce(
      (accumulator: number, currentValue: number) => accumulator + currentValue,
      0,
    );
}
