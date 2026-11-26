package dev.gpui.mobile;

import android.app.NativeActivity;
import android.content.pm.PackageManager;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.TextWatcher;
import android.view.KeyEvent;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;

/** NativeActivity with a UI-thread InputConnection for multistage IMEs. */
public class GpuiInputActivity extends NativeActivity {
    public static final int TEXT_MODE_PROSE = 0;
    public static final int TEXT_MODE_TITLE = 1;
    public static final int TEXT_MODE_SEARCH = 2;
    public static final int TEXT_MODE_PLAIN = 3;

    private InputProxy input;
    private int desiredTextMode = TEXT_MODE_PROSE;
    private int activeInputType = -1;
    private String pendingBefore = "";
    private String pendingAfter = "";

    @Override protected void onCreate(Bundle state) {
        // NativeActivity's dlopen alone does not register JNI native methods.
        try {
            String library = getPackageManager().getActivityInfo(getComponentName(),
                    PackageManager.GET_META_DATA).metaData.getString("android.app.lib_name");
            if (library != null) System.loadLibrary(library);
        } catch (PackageManager.NameNotFoundException error) {
            throw new IllegalStateException(error);
        }
        super.onCreate(state);
    }

    private static int inputTypeForMode(int keyboardType, int textMode) {
        switch (keyboardType) {
            case 1: return InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS;
            case 2: return InputType.TYPE_CLASS_PHONE;
            case 3: return InputType.TYPE_CLASS_NUMBER;
            case 4: return InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI;
            case 5: return InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_DECIMAL;
            default:
                switch (textMode) {
                    case TEXT_MODE_TITLE:
                        return InputType.TYPE_CLASS_TEXT
                                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                                | InputType.TYPE_TEXT_FLAG_CAP_WORDS
                                | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
                    case TEXT_MODE_SEARCH:
                        return InputType.TYPE_CLASS_TEXT
                                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                                | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
                    case TEXT_MODE_PLAIN:
                        return InputType.TYPE_CLASS_TEXT
                                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                                | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
                    case TEXT_MODE_PROSE:
                    default:
                        return InputType.TYPE_CLASS_TEXT
                                | InputType.TYPE_TEXT_FLAG_MULTI_LINE
                                | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES
                                | InputType.TYPE_TEXT_FLAG_AUTO_CORRECT;
                }
        }
    }

    public void gpuiShowKeyboard(int keyboardType, long session) {
        runOnUiThread(() -> {
            if (input == null) {
                input = new InputProxy();
                input.setAlpha(0f);
                input.setPadding(0, 0, 0, 0);
                addContentView(input, new ViewGroup.LayoutParams(1, 1));
            }
            int type = inputTypeForMode(keyboardType, desiredTextMode);
            input.applyInputTypeAndSeed(session, type, pendingBefore, pendingAfter);
            activeInputType = type;
            input.setImeOptions(EditorInfo.IME_FLAG_NO_EXTRACT_UI);
            input.requestFocus();
            InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
            if (imm != null) {
                imm.restartInput(input);
                imm.showSoftInput(input, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    public void gpuiConfigureIme(int mode, String beforeCursor, String afterCursor, boolean restartIfOpen) {
        final String before = beforeCursor != null ? beforeCursor : "";
        final String after = afterCursor != null ? afterCursor : "";
        runOnUiThread(() -> {
            desiredTextMode = mode;
            pendingBefore = before;
            pendingAfter = after;
            if (input != null && input.hasFocus()) {
                int type = inputTypeForMode(0, desiredTextMode);
                boolean typeChanged = (type != activeInputType);
                boolean ctxChanged = !input.matchesContext(before, after);
                if (typeChanged || (restartIfOpen && ctxChanged && !input.isComposingActive())) {
                    input.applyInputTypeAndSeed(input.currentSession(), type, before, after);
                    activeInputType = type;
                    InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
                    if (imm != null) imm.restartInput(input);
                }
            }
        });
    }

    public void gpuiSyncImeContext(String beforeCursor, String afterCursor) {
        final String before = beforeCursor != null ? beforeCursor : "";
        final String after = afterCursor != null ? afterCursor : "";
        runOnUiThread(() -> {
            pendingBefore = before;
            pendingAfter = after;
            if (input != null && input.hasFocus()
                    && !input.isComposingActive()
                    && !input.matchesContext(before, after)) {
                input.seedContextKeepingSession(before, after);
                InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
                if (imm != null) imm.restartInput(input);
            }
        });
    }

    public void gpuiHideKeyboard(long session) {
        runOnUiThread(() -> {
            if (input == null) return;
            input.reset(session);
            InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
            if (imm != null) imm.hideSoftInputFromWindow(input.getWindowToken(), 0);
            input.clearFocus();
        });
    }

    public void gpuiResetComposition(long session) {
        runOnUiThread(() -> {
            if (input == null) return;
            input.reset(session);
            InputMethodManager imm = (InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
            if (imm != null) imm.restartInput(input);
        });
    }

    private final class InputProxy extends EditText {
        private long session;
        private int depth;
        private boolean marked;
        private String prevCommitted = "";
        private int prevCursor = 0;

        InputProxy() {
            super(GpuiInputActivity.this);
            addTextChangedListener(new TextWatcher() {
                public void beforeTextChanged(CharSequence s, int start, int count, int after) {}
                public void onTextChanged(CharSequence s, int start, int before, int count) {}
                public void afterTextChanged(Editable text) {
                    // Hardware keyboards edit the widget directly, outside its
                    // InputConnection. IME mutations are batched by depth below.
                    if (depth == 0) { depth++; endEdit(); }
                }
            });
        }

        long currentSession() {
            return session;
        }

        boolean isComposingActive() {
            return marked || BaseInputConnection.getComposingSpanStart(getText()) >= 0;
        }

        boolean matchesContext(String before, String after) {
            return prevCommitted.equals(before + after) && prevCursor == before.length();
        }

        @Override public boolean onKeyDown(int code, KeyEvent event) {
            if (code == KeyEvent.KEYCODE_DEL
                    && !marked
                    && Selection.getSelectionStart(getText()) <= 0
                    && Selection.getSelectionEnd(getText()) <= 0) {
                nativeIme(session, 3, "", 1, 0);
                return true;
            }
            return super.onKeyDown(code, event);
        }

        void applyInputTypeAndSeed(long nextSession, int type, String before, String after) {
            depth++;
            try {
                session = nextSession;
                marked = false;
                setInputType(type);
                String combined = before + after;
                setText(combined);
                int cur = Math.max(0, Math.min(combined.length(), before.length()));
                Selection.setSelection(getText(), cur);
                prevCommitted = combined;
                prevCursor = cur;
            } finally {
                depth = 0;
            }
        }

        void seedContextKeepingSession(String before, String after) {
            depth++;
            try {
                marked = false;
                String combined = before + after;
                setText(combined);
                int cur = Math.max(0, Math.min(combined.length(), before.length()));
                Selection.setSelection(getText(), cur);
                prevCommitted = combined;
                prevCursor = cur;
            } finally {
                depth = 0;
            }
        }

        void reset(long nextSession) {
            session = nextSession;
            seedContextKeepingSession("", "");
        }

        private void endEdit() {
            if (--depth != 0) return;
            Editable text = getText();
            String full = text.toString();
            int compStart = BaseInputConnection.getComposingSpanStart(text);
            int compEnd = BaseInputConnection.getComposingSpanEnd(text);
            boolean composing = compStart >= 0 && compEnd >= compStart && compEnd <= full.length();
            int selStart = Math.max(0, Math.min(full.length(), Selection.getSelectionStart(text)));
            int selEnd = Math.max(0, Math.min(full.length(), Selection.getSelectionEnd(text)));

            String curCommitted;
            int curAnchor;
            String compText = "";
            int relSelStart = 0;
            int relSelEnd = 0;
            if (composing) {
                curCommitted = full.substring(0, compStart) + full.substring(compEnd);
                curAnchor = compStart;
                compText = full.substring(compStart, compEnd);
                relSelStart = Math.max(0, Math.min(compText.length(), selStart - compStart));
                relSelEnd = Math.max(0, Math.min(compText.length(), selEnd - compStart));
            } else {
                curCommitted = full;
                curAnchor = selEnd;
            }

            int pCur = Math.max(0, Math.min(prevCommitted.length(), prevCursor));
            int cCur = Math.max(0, Math.min(curCommitted.length(), curAnchor));
            int maxPre = Math.min(pCur, cCur);
            int pre = 0;
            while (pre < maxPre && prevCommitted.charAt(pre) == curCommitted.charAt(pre)) {
                pre++;
            }
            int pAfter = prevCommitted.length() - pCur;
            int cAfter = curCommitted.length() - cCur;
            int maxSuf = Math.min(Math.min(pAfter, cAfter), Math.min(prevCommitted.length(), curCommitted.length()) - pre);
            int suf = 0;
            while (suf < maxSuf && prevCommitted.charAt(prevCommitted.length() - 1 - suf) == curCommitted.charAt(curCommitted.length() - 1 - suf)) {
                suf++;
            }
            if (pre > 0 && Character.isHighSurrogate(prevCommitted.charAt(pre - 1))) {
                pre--;
            }
            if (suf > 0 && Character.isLowSurrogate(prevCommitted.charAt(prevCommitted.length() - suf))) {
                suf--;
            }
            int delBefore = pCur - pre;
            int delAfter = pAfter - suf;
            String inserted = curCommitted.substring(pre, curCommitted.length() - suf);
            boolean committedChanged = delBefore > 0 || delAfter > 0 || !inserted.isEmpty();

            if (marked) {
                if (!composing) {
                    if (delBefore == 0 && delAfter == 0) {
                        nativeIme(session, 1, inserted, inserted.length(), inserted.length());
                    } else {
                        nativeIme(session, 1, "", 0, 0);
                        nativeIme(session, 2, "", delBefore, delAfter);
                        if (!inserted.isEmpty()) {
                            nativeIme(session, 1, inserted, inserted.length(), inserted.length());
                        }
                    }
                    marked = false;
                } else {
                    if (committedChanged) {
                        if (delBefore == 0 && delAfter == 0) {
                            nativeIme(session, 1, inserted, inserted.length(), inserted.length());
                        } else {
                            nativeIme(session, 1, "", 0, 0);
                            nativeIme(session, 2, "", delBefore, delAfter);
                            if (!inserted.isEmpty()) {
                                nativeIme(session, 1, inserted, inserted.length(), inserted.length());
                            }
                        }
                    }
                    nativeIme(session, 0, compText, relSelStart, relSelEnd);
                    marked = true;
                }
            } else {
                if (committedChanged) {
                    if (delBefore > 0 || delAfter > 0) {
                        nativeIme(session, 2, "", delBefore, delAfter);
                    }
                    if (!inserted.isEmpty()) {
                        nativeIme(session, 1, inserted, inserted.length(), inserted.length());
                    }
                }
                if (composing) {
                    nativeIme(session, 0, compText, relSelStart, relSelEnd);
                    marked = true;
                }
            }

            if (committedChanged || composing || marked) {
                prevCommitted = curCommitted;
                prevCursor = curAnchor;
            } else if (selStart != prevCursor || selEnd != prevCursor) {
                depth++;
                try {
                    Selection.setSelection(text, Math.min(prevCursor, text.length()));
                } finally {
                    depth--;
                }
            }

            if (!composing && !marked && text.length() > 320) {
                int keepStart = Math.max(0, prevCursor - 180);
                int keepEnd = Math.min(text.length(), prevCursor + 64);
                if (keepStart > 0 && keepStart < text.length() && Character.isLowSurrogate(text.charAt(keepStart))) {
                    keepStart++;
                }
                if (keepEnd > 0 && keepEnd < text.length() && Character.isLowSurrogate(text.charAt(keepEnd))) {
                    keepEnd--;
                }
                depth++;
                try {
                    CharSequence slice = text.subSequence(keepStart, keepEnd);
                    text.replace(0, text.length(), slice);
                    prevCursor -= keepStart;
                    prevCommitted = text.toString();
                    Selection.setSelection(text, Math.max(0, Math.min(text.length(), prevCursor)));
                } finally {
                    depth--;
                }
            }
        }

        @Override public boolean onKeyPreIme(int code, KeyEvent event) {
            if (code == KeyEvent.KEYCODE_BACK && event.getAction() == KeyEvent.ACTION_UP) {
                nativeIme(session, 5, "", 0, 0);
            }
            return super.onKeyPreIme(code, event);
        }

        @Override public InputConnection onCreateInputConnection(EditorInfo info) {
            InputConnection connection = super.onCreateInputConnection(info);
            if (connection == null) return null;
            final long connectionSession = session;
            return new InputConnectionWrapper(connection, false) {
                @Override public boolean beginBatchEdit() {
                    if (connectionSession != session) return false;
                    depth++;
                    return super.beginBatchEdit();
                }
                @Override public boolean endBatchEdit() {
                    if (connectionSession != session) return false;
                    boolean result = super.endBatchEdit();
                    if (depth > 0) endEdit();
                    return result;
                }
                @Override public boolean setComposingText(CharSequence text, int cursor) {
                    if (connectionSession != session) return false;
                    depth++;
                    try { return super.setComposingText(text, cursor); }
                    finally { endEdit(); }
                }
                @Override public boolean setComposingRegion(int start, int end) {
                    if (connectionSession != session) return false;
                    depth++;
                    try { return super.setComposingRegion(start, end); }
                    finally { endEdit(); }
                }
                @Override public boolean finishComposingText() {
                    if (connectionSession != session) return false;
                    depth++;
                    try { return super.finishComposingText(); }
                    finally { endEdit(); }
                }
                @Override public boolean commitText(CharSequence text, int cursor) {
                    if (connectionSession != session) return false;
                    depth++;
                    try { return super.commitText(text, cursor); }
                    finally { endEdit(); }
                }
                @Override public boolean setSelection(int start, int end) {
                    if (connectionSession != session) return false;
                    depth++;
                    try { return super.setSelection(start, end); }
                    finally { endEdit(); }
                }
                @Override public boolean deleteSurroundingText(int before, int after) {
                    if (connectionSession != session) return false;
                    Editable t = getText();
                    int s = Math.max(0, Selection.getSelectionStart(t));
                    int e = Math.max(0, Selection.getSelectionEnd(t));
                    if (!marked && s == e) {
                        int inBufBefore = Math.min(Math.max(0, before), s);
                        int inBufAfter = Math.min(Math.max(0, after), t.length() - e);
                        int extraBefore = Math.max(0, before - inBufBefore);
                        int extraAfter = Math.max(0, after - inBufAfter);
                        if (inBufBefore > 0 || inBufAfter > 0) {
                            depth++;
                            try { super.deleteSurroundingText(inBufBefore, inBufAfter); }
                            finally { endEdit(); }
                        }
                        if (extraBefore > 0 || extraAfter > 0) {
                            nativeIme(session, 2, "", extraBefore, extraAfter);
                        }
                        return true;
                    }
                    depth++;
                    try { return super.deleteSurroundingText(before, after); }
                    finally { endEdit(); }
                }
                @Override public boolean deleteSurroundingTextInCodePoints(int before, int after) {
                    if (connectionSession != session) return false;
                    Editable t = getText();
                    int s = Math.max(0, Selection.getSelectionStart(t));
                    int e = Math.max(0, Selection.getSelectionEnd(t));
                    if (!marked && s == e) {
                        int cpBefore = Character.codePointCount(t, 0, s);
                        int cpAfter = Character.codePointCount(t, e, t.length());
                        int inBufBefore = Math.min(Math.max(0, before), cpBefore);
                        int inBufAfter = Math.min(Math.max(0, after), cpAfter);
                        int extraBefore = Math.max(0, before - inBufBefore);
                        int extraAfter = Math.max(0, after - inBufAfter);
                        if (inBufBefore > 0 || inBufAfter > 0) {
                            depth++;
                            try { super.deleteSurroundingTextInCodePoints(inBufBefore, inBufAfter); }
                            finally { endEdit(); }
                        }
                        if (extraBefore > 0 || extraAfter > 0) {
                            nativeIme(session, 3, "", extraBefore, extraAfter);
                        }
                        return true;
                    }
                    depth++;
                    try { return super.deleteSurroundingTextInCodePoints(before, after); }
                    finally { endEdit(); }
                }
                @Override public boolean sendKeyEvent(KeyEvent event) {
                    if (connectionSession != session) return false;
                    if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                        if (event.getAction() == KeyEvent.ACTION_DOWN) deleteSurroundingText(1, 0);
                        return true;
                    }
                    if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                        if (event.getAction() == KeyEvent.ACTION_DOWN) commitText("\n", 1);
                        return true;
                    }
                    return super.sendKeyEvent(event);
                }
                @Override public boolean performEditorAction(int action) {
                    if (connectionSession != session) return false;
                    if (action == EditorInfo.IME_ACTION_DONE) {
                        finishComposingText();
                        nativeIme(session, 4, "", 0, 0);
                        return true;
                    }
                    return commitText("\n", 1);
                }
            };
        }
    }

    private static native void nativeIme(long session, int kind, String text, int start, int end);
}
