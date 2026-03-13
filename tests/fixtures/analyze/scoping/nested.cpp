void Navigate() {
    // https://html.spec.whatwg.org/#navigate
    // Step 1. Let cspNavigationType be form-submission
    auto csp = GetCSPNavType();

    // Step 2. If url is about:blank:
    if (IsAboutBlank(url)) {
        // https://dom.spec.whatwg.org/#concept-tree
        // Step 1. Let root be the tree root
        auto root = GetRoot();
        // Step 2. Return root
        return root;
    }

    // Step 3. If url is about:blank, then return
    Continue();
}
