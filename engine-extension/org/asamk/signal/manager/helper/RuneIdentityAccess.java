// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.manager.helper;

import org.asamk.signal.manager.Manager;
import org.asamk.signal.manager.api.RecipientIdentifier;
import org.asamk.signal.manager.api.UnregisteredRecipientException;
import org.asamk.signal.manager.internal.ManagerImpl;

/** Narrow access to the pinned engine's helper; never falls back to trust-all. */
public final class RuneIdentityAccess {
    private RuneIdentityAccess() {}
    public static boolean unverify(Manager manager, RecipientIdentifier.Single recipient, String expectedSafetyNumber)
            throws ReflectiveOperationException, UnregisteredRecipientException {
        if (!(manager instanceof ManagerImpl)) throw new IllegalArgumentException("Unsupported engine manager");
        var contextField = ManagerImpl.class.getDeclaredField("context");
        contextField.setAccessible(true);
        var context = (Context) contextField.get(manager);
        var id = context.getRecipientHelper().resolveRecipient(recipient);
        return context.getIdentityHelper().trustIdentityUnverifiedSafetyNumber(id, expectedSafetyNumber);
    }
}
