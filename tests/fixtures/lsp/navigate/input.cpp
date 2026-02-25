// https://html.spec.whatwg.org/#navigate
void DoNavigate(bool userInvolvement) {
  // Step 1. Let cspNavigationType be form-submission
  auto cspNavigationType = GetCSPNavType();

  // Step 2. Let sourceSnapshotParams be the result of snapshotting
  auto params = SnapshotParams();

  // Step 3. If url is about:blank, then return
  if (IsAboutBlank(url)) {
    return;
  }

  // Step 99. Nonexistent step
  DoSomething();
}
