import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release as r


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'src-tauri').mkdir()
        for name in ('package.json', 'src-tauri/tauri.conf.json'):
            (self.root / name).write_text('{"version":"0.1.2"}')
        (self.root / 'Cargo.toml').write_text('[workspace.package]\nversion = "0.1.2"\n')
        (self.root / 'Cargo.lock').write_text(''.join(
            f'[[package]]\nname = "{name}"\nversion = "0.1.2"\n'
            for name in (
                'skill-studio', 'skill-studio-cli', 'skill-studio-core', 'skill-studio-mcp',
                'skill-studio-remote', 'skill-studio-service', 'dependency')))
        (self.root / 'CHANGELOG.md').write_text('## v0.1.2\nFixed bugs.\n## v0.1.1\nOlder.\n')

    def assets(self):
        return [{'name': f'Skill.Studio_0.1.2_{suffix}', 'size': 1} for suffix in
                ('universal.dmg', 'x64-setup.exe', 'x64_en-US.msi', 'amd64.AppImage', 'amd64.deb')]

    def test_versions_and_notes(self):
        self.assertEqual(r.verify(self.root, 'v0.1.2'), '0.1.2')
        self.assertEqual(r.notes(self.root, '0.1.2'), 'Fixed bugs.')
        with self.assertRaises(ValueError):
            r.verify(self.root, 'v0.1.3')
        r.set_version(self.root, '0.1.3')
        self.assertEqual(set(r.versions(self.root).values()), {'0.1.3'})
        self.assertIn('name = "dependency"\nversion = "0.1.2"', (self.root / 'Cargo.lock').read_text())
        with self.assertRaises(ValueError):
            r.verify(self.root)

    def test_every_workspace_crate_is_versioned(self):
        (self.root / 'crates/new').mkdir(parents=True)
        (self.root / 'crates/new/Cargo.toml').write_text('[package]\nname = "skill-studio-new"\n')
        (self.root / 'Cargo.toml').write_text(
            '[workspace]\nmembers = ["crates/new"]\n[workspace.package]\nversion = "0.1.2"\n')
        with self.assertRaisesRegex(ValueError, 'skill-studio-new'):
            r.verify(self.root)

    def test_invalid_version_does_not_write(self):
        before = (self.root / 'package.json').read_text()
        for value in ('v0.1.3', '0.01.3', '1.2', '1.2.3-beta'):
            with self.assertRaises(ValueError):
                r.set_version(self.root, value)
        self.assertEqual(before, (self.root / 'package.json').read_text())

    def test_empty_duplicate_placeholder_notes(self):
        for text in ('## v0.1.2\n## v0.1.1\nOld\n', '## v0.1.2\nTODO\n',
                     '## v0.1.2\nA\n## v0.1.2\nB\n'):
            (self.root / 'CHANGELOG.md').write_text(text)
            with self.assertRaises(ValueError):
                r.notes(self.root, '0.1.2')

    def test_assets(self):
        r.check_assets(self.assets(), '0.1.2')
        for assets in (self.assets()[:-1], self.assets() * 2,
                       [dict(a, size=0) for a in self.assets()]):
            with self.assertRaises(ValueError):
                r.check_assets(assets, '0.1.2')
        with self.assertRaises(ValueError):
            r.check_assets(self.assets(), '0.1.3')

    def test_ci_requires_latest_exact_main_push(self):
        good = dict(databaseId=1, headSha='abc', headBranch='main', event='push',
                    status='completed', conclusion='success', url='url')
        self.assertEqual(r.successful_run([good], 'abc', 'main'), good)
        for runs in ([dict(good, headSha='other')], [dict(good, event='pull_request')],
                     [dict(good, headBranch='branch')],
                     [good, dict(good, databaseId=2, status='in_progress', conclusion=None)]):
            with self.assertRaises(ValueError):
                r.successful_run(runs, 'abc', 'main')

    def test_workflow_query_filters_exact_sha_after_listing_branch(self):
        good = dict(databaseId=1, headSha='abc', headBranch='main', event='push',
                    status='completed', conclusion='success', url='url')
        with patch.object(r, 'gh', return_value=[good]) as gh:
            self.assertEqual(r.workflow_run('ci.yml', 'abc', 'main'), good)
        args = gh.call_args.args
        self.assertNotIn('--commit', args)
        self.assertEqual(args[args.index('--branch') + 1], 'main')

    @patch.object(r, 'verify', return_value='0.1.2')
    def test_dirty_tree_blocks_tag_mutation(self, verify):
        with patch.object(r, 'run', side_effect=['main', ' M file']) as run:
            with self.assertRaises(ValueError):
                r.publish_tag('0.1.2')
            self.assertFalse(any('tag' in c.args or 'push' in c.args for c in run.call_args_list))

    @patch.object(r, 'verify', return_value='0.1.2')
    def test_published_release_retry_does_not_mutate(self, verify):
        for a in self.assets():
            (self.root / a['name']).write_text('x')
        artifact_dir = self.root / 'artifacts'
        artifact_dir.mkdir()
        for a in self.assets():
            (self.root / a['name']).rename(artifact_dir / a['name'])
        with patch.object(r, 'gh', side_effect=[
            [{'tagName': 'v0.1.2'}],
            {'isDraft': False, 'isPrerelease': False, 'assets': self.assets()},
        ]), patch.object(r, 'run') as run:
            r.publish_assets(artifact_dir)
            run.assert_not_called()

    @patch.object(r, 'verify', return_value='0.1.2')
    def test_incomplete_upload_never_publishes(self, verify):
        folder = self.root / 'artifacts'
        folder.mkdir()
        for a in self.assets():
            (folder / a['name']).write_text('x')
        with patch.object(r, 'gh', side_effect=[
            [{'tagName': 'v0.1.2'}],
            {'isDraft': True, 'isPrerelease': False, 'assets': []},
            {'assets': self.assets()[:-1]},
        ]), patch.object(r, 'run') as run:
            with self.assertRaises(ValueError):
                r.publish_assets(folder)
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[:3], ('gh', 'release', 'upload'))

    @patch.object(r, 'verify', return_value='0.1.2')
    def test_failed_ci_never_creates_or_pushes_tag(self, verify):
        with patch.object(r, 'run', side_effect=[
            'main', '', f'git@github.com:{r.REPO}.git', '', 'sha', 'sha',
        ]) as run, patch.object(r, 'ci_gate', side_effect=ValueError('CI failed')):
            with self.assertRaises(ValueError):
                r.publish_tag('0.1.2')
            self.assertFalse(any('tag' in c.args or 'push' in c.args for c in run.call_args_list))

    def workflow(self, **changes):
        return dict(dict(databaseId=42, headSha='abc', headBranch='main',
                         event='push', status='completed', conclusion='success', url='url'), **changes)

    def test_wait_discovers_then_reuses_run_without_repeating_output(self):
        pending = self.workflow(status='queued', conclusion=None)
        output = io.StringIO()
        with patch.object(r, 'workflow_runs', side_effect=[[], [pending]]) as listing, \
                patch.object(r, 'gh', side_effect=[pending, self.workflow()]) as gh, \
                patch.object(r.time, 'sleep'), contextlib.redirect_stdout(output):
            self.assertEqual(r.wait_workflow('ci.yml', 'abc', 'main')['databaseId'], 42)
        self.assertEqual(listing.call_count, 2)
        self.assertEqual([c.args[:3] for c in gh.call_args_list], [('run', 'view', '42')] * 2)
        self.assertEqual(len(output.getvalue().splitlines()), 3)

    def test_wait_failure_and_cancel_stop_immediately(self):
        for conclusion in ('failure', 'cancelled', 'timed_out'):
            with patch.object(r, 'workflow_runs', return_value=[self.workflow(conclusion=conclusion)]), \
                    patch.object(r.time, 'sleep') as sleep:
                with self.assertRaisesRegex(ValueError, 'url'):
                    r.wait_workflow('ci.yml', 'abc', 'main')
                sleep.assert_not_called()

    def test_wait_missing_workflow_times_out_without_mutation(self):
        with patch.object(r, 'workflow_runs', return_value=[]), \
                patch.object(r.time, 'monotonic', side_effect=[0, 0, 301]), \
                patch.object(r, 'run') as run:
            with self.assertRaisesRegex(ValueError, '流水线未出现'):
                r.wait_workflow('ci.yml', 'abc', 'main')
            run.assert_not_called()

    def test_wait_api_error_is_not_missing_workflow(self):
        with patch.object(r, 'workflow_runs', side_effect=OSError('offline')), \
                patch.object(r.time, 'sleep') as sleep:
            with self.assertRaisesRegex(OSError, 'offline'):
                r.wait_workflow('ci.yml', 'abc', 'main')
            sleep.assert_not_called()

    def test_head_change_during_wait_never_tags(self):
        with patch.object(r, 'publication_context', side_effect=['abc', 'def']), \
                patch.object(r, 'wait_workflow'), patch.object(r, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'HEAD'):
                r.publish_tag('0.1.2', wait=True)
            run.assert_not_called()

    def test_wait_failed_ci_never_tags(self):
        with patch.object(r, 'publication_context', return_value='abc'), \
                patch.object(r, 'wait_workflow', side_effect=ValueError('CI failed')), \
                patch.object(r, 'run') as run:
            with self.assertRaises(ValueError):
                r.publish_tag('0.1.2', wait=True)
            run.assert_not_called()

    def test_release_stops_before_final_verification_on_workflow_failure(self):
        with patch.object(r, 'publish_tag', return_value='abc'), \
                patch.object(r, 'wait_workflow', side_effect=ValueError('Homebrew failed')), \
                patch.object(r, 'verify_release') as verify:
            with self.assertRaisesRegex(ValueError, 'Homebrew'):
                r.release('0.1.2')
            verify.assert_not_called()

    def test_release_verifies_once_after_success(self):
        calls = []
        with patch.object(r, 'publish_tag', side_effect=lambda *a, **kw: calls.append('tag') or 'abc'), \
                patch.object(r, 'wait_workflow', side_effect=lambda *a: calls.append('wait')), \
                patch.object(r, 'verify_release', side_effect=lambda *a: calls.append('verify') or 'url'):
            r.release('0.1.2')
        self.assertEqual(calls, ['tag', 'wait', 'verify'])

    def test_wait_running_workflow_times_out(self):
        with patch.object(r, 'workflow_runs', return_value=[self.workflow(status='in_progress')]), \
                patch.object(r.time, 'monotonic', side_effect=[0, 0, 3601]):
            with self.assertRaisesRegex(ValueError, '超时：url'):
                r.wait_workflow('ci.yml', 'abc', 'main')

    def test_existing_tag_is_reused_without_creation(self):
        with patch.object(r, 'publication_context', return_value='abc'), \
                patch.object(r, 'wait_workflow'), \
                patch.object(r, 'run', side_effect=['v0.1.2', 'abc', '']) as run:
            r.publish_tag('0.1.2', wait=True)
        self.assertFalse(any(c.args[:3] == ('git', 'tag', '-a') for c in run.call_args_list))
        self.assertEqual(run.call_args.args, ('git', 'push', 'origin', 'refs/tags/v0.1.2'))

    def test_conflicting_tag_is_never_pushed(self):
        with patch.object(r, 'publication_context', return_value='abc'), \
                patch.object(r, 'wait_workflow'), \
                patch.object(r, 'run', side_effect=['v0.1.2', 'different']) as run:
            with self.assertRaisesRegex(ValueError, '禁止覆盖'):
                r.publish_tag('0.1.2', wait=True)
        self.assertFalse(any('push' in c.args for c in run.call_args_list))

    def test_mcp_version_mismatch_is_detected(self):
        p = self.root / 'Cargo.lock'
        p.write_text(p.read_text().replace('name = "skill-studio-mcp"\nversion = "0.1.2"',
                                          'name = "skill-studio-mcp"\nversion = "0.1.1"'))
        with self.assertRaisesRegex(ValueError, '版本不一致'):
            r.verify(self.root)


if __name__ == '__main__':
    unittest.main()
