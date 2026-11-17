// SPDX-License-Identifier: 0BSD
export const packageDirectory = new URL('../', import.meta.url);
export const conformanceDirectory = new URL('../', packageDirectory);
export const testdataRelativePath = '../../testdata/';
export const testdataDirectory = new URL(testdataRelativePath, packageDirectory);
