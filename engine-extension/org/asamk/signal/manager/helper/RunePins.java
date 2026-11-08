// SPDX-License-Identifier: GPL-3.0-only
package org.asamk.signal.manager.helper;

import java.io.IOException;
import java.lang.reflect.Field;
import java.sql.Connection;
import java.sql.SQLException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.WeakHashMap;
import org.asamk.signal.manager.Manager;
import org.asamk.signal.manager.api.GroupId;
import org.asamk.signal.manager.api.RecipientIdentifier;
import org.asamk.signal.manager.internal.ManagerImpl;
import org.asamk.signal.manager.jobs.SyncStorageJob;
import org.asamk.signal.manager.storage.SignalAccount;
import org.asamk.signal.manager.storage.groups.GroupInfoV2;
import org.signal.core.models.ServiceId;
import org.signal.libsignal.zkgroup.groups.GroupMasterKey;
import org.signal.libsignal.zkgroup.groups.GroupSecretParams;
import org.whispersystems.signalservice.api.storage.SignalAccountRecord;
import org.whispersystems.signalservice.api.storage.SignalStorageManifest;
import org.whispersystems.signalservice.api.storage.SignalStorageRecord;
import org.whispersystems.signalservice.internal.storage.protos.AccountRecord;
import org.whispersystems.signalservice.internal.storage.protos.AccountRecord.PinnedConversation;
import okio.ByteString;

/** Pin intents are durable in the engine's account DB and acknowledged only after a
 * successful remote write or a manifest proving our unique storage ID is present.
 * AccountRecord builders preserve unknown fields and unrelated phone preferences.
 */
public final class RunePins {
    private RunePins() {}
    private static final String TABLE="rune_pin_operations";
    private static final Map<SignalAccount,Boolean> ACTIVE=new WeakHashMap<>();
    public record Intent(long id, PinnedConversation target, boolean pinned) {}
    public static Context context(Manager manager) throws ReflectiveOperationException {
        if (!(manager instanceof ManagerImpl)) throw new IllegalArgumentException("Unsupported engine manager");
        Field field=ManagerImpl.class.getDeclaredField("context");field.setAccessible(true);return (Context)field.get(manager);
    }
    public static boolean exists(Connection db) throws SQLException {
        try(var stmt=db.prepareStatement("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?")) {stmt.setString(1,TABLE);try(var rows=stmt.executeQuery()){return rows.next();}}
    }
    public static void initialize(Connection db) throws SQLException {
        try(var stmt=db.createStatement()) {stmt.executeUpdate("CREATE TABLE IF NOT EXISTS rune_pin_operations (id INTEGER PRIMARY KEY NOT NULL, target BLOB NOT NULL, pinned INTEGER NOT NULL, status INTEGER NOT NULL DEFAULT 0, request_key TEXT)");}
    }
    public enum RecordStatus {NEW,PENDING,DONE}
    public static RecordStatus record(Connection db,Intent intent) throws SQLException {
        initialize(db);
        try(var existing=db.prepareStatement("SELECT target,pinned,status FROM rune_pin_operations WHERE id=?")) {
            existing.setLong(1,intent.id());try(var row=existing.executeQuery()) {
                if(row.next()) {
                    if(!Arrays.equals(row.getBytes(1),intent.target().encode()) || row.getBoolean(2)!=intent.pinned()) throw new IllegalArgumentException("Pin operation ID was reused with different content");
                    return row.getInt(3)==1?RecordStatus.DONE:RecordStatus.PENDING;
                }
            }
        }
        try(var insert=db.prepareStatement("INSERT INTO rune_pin_operations(id,target,pinned) VALUES(?,?,?)")) {insert.setLong(1,intent.id());insert.setBytes(2,intent.target().encode());insert.setBoolean(3,intent.pinned());insert.executeUpdate();}
        return RecordStatus.NEW;
    }
    public static boolean acknowledge(Connection db,byte[] currentStorageId,byte[] writtenStorageId) throws SQLException {
        if(!Arrays.equals(currentStorageId,writtenStorageId)) return false;
        finish(db);return true;
    }
    public static List<Intent> pending(Connection db) throws SQLException {
        if(!exists(db)) return List.of();
        var result=new ArrayList<Intent>();
        try(var stmt=db.prepareStatement("SELECT id,target,pinned FROM rune_pin_operations WHERE status=0 ORDER BY id");var rows=stmt.executeQuery()) {
            while(rows.next()) {try {result.add(new Intent(rows.getLong(1),PinnedConversation.ADAPTER.decode(rows.getBytes(2)),rows.getBoolean(3)));}catch(IOException e){throw new SQLException("Invalid stored pin intent",e);}}
        }return result;
    }
    public static List<PinnedConversation> overlay(Connection db,List<PinnedConversation> base) throws SQLException {return apply(base,pending(db));}
    public static List<PinnedConversation> overlayUnchecked(Connection db,List<PinnedConversation> base) {
        try{return overlay(db,base);}catch(SQLException e){throw new IllegalStateException("Cannot load durable pin intents",e);}
    }
    public static List<PinnedConversation> apply(List<PinnedConversation> base,List<Intent> intents) {
        var result=new ArrayList<>(base);
        for(var intent:intents) {
            if(!intent.pinned()) {result.removeIf(pin->same(pin,intent.target()));}
            else if(result.stream().noneMatch(pin->same(pin,intent.target()))) {result.add(intent.target());}
        }return List.copyOf(result);
    }
    private static boolean has(ByteString value) {return value!=null && value.size()>0;}
    public static boolean same(PinnedConversation a,PinnedConversation b) {
        if(a.contact!=null && b.contact!=null) {
            var first=ServiceId.parseOrNull(a.contact.serviceId,a.contact.serviceIdBinary);
            var second=ServiceId.parseOrNull(b.contact.serviceId,b.contact.serviceIdBinary);
            if(first!=null && second!=null) return first.equals(second);
            return !a.contact.e164.isEmpty() && a.contact.e164.equals(b.contact.e164);
        }
        if(has(a.groupMasterKey) && has(b.groupMasterKey)) return a.groupMasterKey.equals(b.groupMasterKey);
        if(has(a.legacyGroupId) && has(b.legacyGroupId)) return a.legacyGroupId.equals(b.legacyGroupId);
        return a.equals(b);
    }
    private static PinnedConversation target(Context context,String recipient,String group) throws Exception {
        if(group!=null) {
            var id=GroupId.fromBase64(group);var info=context.getAccount().getGroupStore().getGroup(id);
            if(info==null) throw new IllegalArgumentException("Group is unknown");
            if(info instanceof GroupInfoV2 modern) return new PinnedConversation.Builder().groupMasterKey(ByteString.of(modern.getMasterKey().serialize())).build();
            return new PinnedConversation.Builder().legacyGroupId(ByteString.of(id.serialize())).build();
        }
        var single=org.asamk.signal.util.CommandUtil.getSingleRecipientIdentifier(recipient,context.getAccount().getNumber());
        var id=context.getRecipientHelper().resolveRecipient(single);
        try(var db=context.getAccount().getAccountDatabase().getConnection()) {
        var address=context.getAccount().getRecipientStore().getRecipient(db,id).getAddress();
        var contact=new PinnedConversation.Contact.Builder().e164(address.number().orElse(""));
        if(address.aci().isPresent()) {var aci=address.aci().get();contact.serviceId(aci.toString()).serviceIdBinary(aci.toByteString());}
        else if(address.pni().isPresent()) {var pni=address.pni().get();contact.serviceId(pni.toString()).serviceIdBinary(pni.toByteString());}
        else if(address.number().isEmpty()) throw new IllegalArgumentException("Recipient has no storage identifier");
        return new PinnedConversation.Builder().contact(contact.build()).build();
        }
    }
    public static Map<String,Object> set(Manager manager,long operation,String recipient,String group,boolean pinned) throws Exception {
        if(operation<=0) throw new IllegalArgumentException("Invalid pin operation ID");
        var context=context(manager);var account=context.getAccount();
        var requestKey=group!=null?"group:"+group:"contact:"+recipient;
        try(var db=account.getAccountDatabase().getConnection()) {
            initialize(db);
            try(var existing=db.prepareStatement("SELECT request_key,pinned,status FROM rune_pin_operations WHERE id=?")) {
                existing.setLong(1,operation);try(var row=existing.executeQuery()) {
                    if(row.next() && row.getString(1)!=null) {
                        if(!requestKey.equals(row.getString(1)) || row.getBoolean(2)!=pinned) throw new IllegalArgumentException("Pin operation ID was reused with different content");
                        var synced=row.getInt(3)==1;schedule(context);return Map.of("operationId",operation,"synced",synced);
                    }
                }
            }
        }
        var target=target(context,recipient,group);
        try(var db=account.getAccountDatabase().getConnection()) {
            db.setAutoCommit(false);initialize(db);
            var recorded=record(db,new Intent(operation,target,pinned));
            if(recorded!=RecordStatus.NEW) {db.commit();schedule(context);return Map.of("operationId",operation,"synced",recorded==RecordStatus.DONE);}
            try(var key=db.prepareStatement("UPDATE rune_pin_operations SET request_key=? WHERE id=?")){key.setString(1,requestKey);key.setLong(2,operation);key.executeUpdate();}
            var self=account.getRecipientStore().getRecipient(db,account.getSelfRecipientId());
            var builder=SignalAccountRecord.Companion.newBuilder(self.getStorageRecord());

            builder.pinnedConversations(overlay(db,builder.build().pinnedConversations));
            account.getRecipientStore().rotateSelfStorageId(db);
            var storageId=account.getRecipientStore().getSelfStorageId(db);
            account.getRecipientStore().storeStorageRecord(db,account.getSelfRecipientId(),storageId,builder.build().encode());
            db.commit();
        }
        schedule(context);return Map.of("operationId",operation,"synced",false);
    }
    public static void schedule(Context context) {
        var account=context.getAccount();synchronized(ACTIVE) {if(ACTIVE.containsKey(account)) return;ACTIVE.put(account,true);}
        context.getJobExecutor().enqueueJob(ignored->{try{new SyncStorageJob().run(context);}finally{synchronized(ACTIVE){ACTIVE.remove(account);}}});
    }
    public static void confirmWritten(SignalAccount account,List<SignalStorageRecord> records) throws IOException {
        try(var db=account.getAccountDatabase().getConnection()) {
            if(!exists(db)) return;db.setAutoCommit(false);
            var current=account.getRecipientStore().getSelfStorageId(db);
            for(var record:records) {if(record.getProto().account!=null && acknowledge(db,current.getRaw(),record.getId().getRaw())) {
                account.getRecipientStore().storeStorageRecord(db,account.getSelfRecipientId(),current,record.getProto().account.encode());
                break;
            }}db.commit();
        } catch(SQLException e) {throw new IOException("Cannot commit pin sync acknowledgement",e);}
    }
    public static void converged(SignalAccount account,SignalStorageManifest manifest) throws IOException {
        try(var db=account.getAccountDatabase().getConnection()) {
            if(!exists(db)) return;db.setAutoCommit(false);
            var current=account.getRecipientStore().getSelfStorageId(db);
            if(manifest.storageIds.contains(current)) acknowledge(db,current.getRaw(),current.getRaw());db.commit();
        } catch(SQLException e) {throw new IOException("Cannot commit converged pin state",e);}
    }
    private static void finish(Connection db) throws SQLException {try(var stmt=db.prepareStatement("UPDATE rune_pin_operations SET status=1 WHERE status=0")){stmt.executeUpdate();}}
    public static Map<String,Object> state(Manager manager) throws Exception {
        var context=context(manager);var account=context.getAccount();
        try(var db=account.getAccountDatabase().getConnection()) {
            var self=account.getRecipientStore().getRecipient(db,account.getSelfRecipientId());
            var proto=SignalAccountRecord.Companion.newBuilder(self.getStorageRecord()).build();
            var pins=overlay(db,proto.pinnedConversations);var visible=new ArrayList<Map<String,Object>>();
            for(var pin:pins) {
                if(pin.contact!=null) {var service=ServiceId.parseOrNull(pin.contact.serviceId,pin.contact.serviceIdBinary);var id=service==null?pin.contact.e164:service.toString();if(!id.isEmpty())visible.add(Map.of("id",id,"group",false));}
                else if(has(pin.legacyGroupId)) visible.add(Map.of("id",GroupId.v1(pin.legacyGroupId.toByteArray()).toBase64(),"group",true));
                else if(has(pin.groupMasterKey)) {var key=new GroupMasterKey(pin.groupMasterKey.toByteArray());var id=GroupSecretParams.deriveFromMasterKey(key).getPublicParams().getGroupIdentifier().serialize();visible.add(Map.of("id",GroupId.v2(id).toBase64(),"group",true));}
            }
            var completed=new ArrayList<Long>();if(exists(db)) {try(var stmt=db.prepareStatement("SELECT id FROM rune_pin_operations WHERE status=1 ORDER BY id DESC LIMIT 128");var rows=stmt.executeQuery()){while(rows.next())completed.add(rows.getLong(1));}}
            return Map.of("supported",true,"ready",self.getStorageRecord()!=null && account.getStorageManifestVersion()>=0,"pins",visible,"count",pins.size(),"completed",completed,"pending",pending(db).size());
        }
    }
}
