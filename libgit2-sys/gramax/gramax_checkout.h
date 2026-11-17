/*
 * Gramax: paths Windows refuses stay in git and never reach the disk.
 *
 * Included into `src/libgit2/checkout.c` by `build.rs`, right before `checkout_get_actions`.
 *
 * Upstream libgit2 stops the whole checkout on a path like `Шаблон "А".rtf` or `con.md` on Windows
 * (`checkout_verify_paths`): a repository with one such file cannot be cloned, pulled or switched there.
 * Here the checkout goes on and leaves that path off the disk: it neither writes nor removes it, and its
 * index entry follows the target, marked assume-unchanged. So the file missing from the disk is not a
 * deletion: status and `git_index_add_all` skip such an entry (`diff_generate.c`, `diff_delta__from_one`).
 *
 * Only the Windows rules are relaxed. A path refused on every platform — `.git`, `..` — still stops
 * the checkout, as upstream.
 */

#define CHECKOUT_ACTION__SKIP_WORKDIR 64

#define GRAMAX_WIN32_REJECT \
	(GIT_FS_PATH_REJECT_BACKSLASH | GIT_FS_PATH_REJECT_TRAILING_DOT | \
	 GIT_FS_PATH_REJECT_TRAILING_SPACE | GIT_FS_PATH_REJECT_TRAILING_COLON | \
	 GIT_FS_PATH_REJECT_DOS_PATHS | GIT_FS_PATH_REJECT_NT_CHARS)

static int (*gramax_win32_rules_callback)(void);

/*
 * Applies the Windows rules on any platform whenever `callback` returns non-zero — so tests on macOS
 * and Linux see what a user on Windows sees. On Windows the rules always apply.
 */
void git_checkout_gramax_win32_rules(int (*callback)(void))
{
	gramax_win32_rules_callback = callback;
}

static unsigned int gramax_win32_reject(void)
{
#ifdef GIT_WIN32
	return GRAMAX_WIN32_REJECT;
#else
	return gramax_win32_rules_callback && gramax_win32_rules_callback() ? GRAMAX_WIN32_REJECT : 0;
#endif
}

static bool gramax_is_refused(git_repository *repo, int action, const git_diff_delta *delta, unsigned int flags)
{
	if ((action & CHECKOUT_ACTION__REMOVE) &&
	    !git_path_is_valid(repo, delta->old_file.path, delta->old_file.mode, flags))
		return true;

	return (action & ~CHECKOUT_ACTION__REMOVE) &&
	       !git_path_is_valid(repo, delta->new_file.path, delta->new_file.mode, flags);
}

/*
 * In place of `checkout_verify_paths`. A path Windows refuses gets an action from the target alone —
 * there is never a file on disk to compare with: gone from the target, its index entry goes too;
 * otherwise the entry becomes the target's blob. Either way the disk is not touched.
 */
static int gramax_verify_paths(checkout_data *data, int *action, git_diff_delta *delta)
{
	unsigned int everywhere = (GIT_PATH_REJECT_WORKDIR_DEFAULTS) & ~GRAMAX_WIN32_REJECT;
	unsigned int win32 = gramax_win32_reject();

	if (!gramax_is_refused(data->repo, *action, delta, everywhere | win32))
		return 0;

	if (gramax_is_refused(data->repo, *action, delta, everywhere) ||
	    S_ISGITLINK(delta->new_file.mode)) {
		git_error_set(GIT_ERROR_CHECKOUT, "cannot checkout to invalid path '%s'", delta->new_file.path);
		return -1;
	}

	if (delta->status == GIT_DELTA_DELETED || S_ISDIR(delta->new_file.mode))
		*action = CHECKOUT_ACTION__REMOVE | CHECKOUT_ACTION__SKIP_WORKDIR;
	else
		*action = CHECKOUT_ACTION__UPDATE_BLOB | CHECKOUT_ACTION__SKIP_WORKDIR;

	return 0;
}

static int checkout_blob(checkout_data *data, const git_diff_file *file);

/* In place of `checkout_blob`: a path left off the disk only gets its index entry. */
static int gramax_checkout_blob(checkout_data *data, const git_diff_file *file, uint32_t action)
{
	git_index_entry entry;

	if ((action & CHECKOUT_ACTION__SKIP_WORKDIR) == 0)
		return checkout_blob(data, file);

	if (!data->index || (data->strategy & GIT_CHECKOUT_DONT_UPDATE_INDEX) != 0)
		return 0;

	memset(&entry, 0, sizeof(entry));
	entry.path = (char *)file->path;
	entry.mode = file->mode;
	entry.flags = GIT_INDEX_ENTRY_VALID;
	git_oid_cpy(&entry.id, &file->id);

	return git_index_add(data->index, &entry);
}
