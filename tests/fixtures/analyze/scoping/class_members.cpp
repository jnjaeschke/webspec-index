class NavigationService {
  // https://html.spec.whatwg.org/#navigate
  void DoNavigate() {
    // Step 1. Let cspNavigationType be form-submission
    auto csp = GetCSPNavType();

    // Step 2. Let sourceSnapshotParams be the result of snapshotting
    auto params = Snapshot();

    // Step 3. If url is about:blank, then return
    if (IsAboutBlank(url)) {
      return;
    }
  }

  void Unrelated() {
    // Step 4. This should NOT be in navigate scope
    DoUnrelated();
  }
};
