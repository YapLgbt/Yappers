const assert = require('node:assert/strict');
const { source } = require('axe-core');
module.exports = async function (page, label) {
  if (!(await page.evaluate(() => !!window.axe))) await page.evaluate(source);
  const findings = await page.evaluate(async () => {
    const result = await window.axe.run(document, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] },
    });
    return result.violations.map((v) => ({
      rule: v.id,
      targets: v.nodes.map((n) => ({ target: n.target, issue: n.failureSummary })),
    }));
  });
  assert.deepEqual(findings, [], label + ' accessibility findings ' + JSON.stringify(findings));
};
