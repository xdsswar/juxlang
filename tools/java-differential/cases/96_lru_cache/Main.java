import java.util.HashMap;

public class Main {
    static class Entry {
        String key;
        int value;
        Entry prev = null;
        Entry next = null;
        Entry(String key, int value) {
            this.key = key;
            this.value = value;
        }
    }

    static class Lru {
        private int capacity;
        private HashMap<String, Entry> map = new HashMap<>();
        private Entry head = null;
        private Entry tail = null;
        int hits = 0;
        int misses = 0;

        Lru(int capacity) { this.capacity = capacity; }

        private void unlink(Entry e) {
            if (e.prev != null) { e.prev.next = e.next; } else { head = e.next; }
            if (e.next != null) { e.next.prev = e.prev; } else { tail = e.prev; }
            e.prev = null;
            e.next = null;
        }

        private void pushFront(Entry e) {
            e.next = head;
            if (head != null) { head.prev = e; }
            head = e;
            if (tail == null) { tail = e; }
        }

        Integer get(String key) {
            if (!map.containsKey(key)) {
                misses = misses + 1;
                return null;
            }
            Entry e = map.get(key);
            unlink(e);
            pushFront(e);
            hits = hits + 1;
            return e.value;
        }

        void put(String key, int value) {
            if (map.containsKey(key)) {
                Entry e = map.get(key);
                e.value = value;
                unlink(e);
                pushFront(e);
                return;
            }
            if (map.size() == capacity) {
                Entry old = tail;
                unlink(old);
                map.remove(old.key);
                System.out.println("evict " + old.key);
            }
            Entry e = new Entry(key, value);
            map.put(key, e);
            pushFront(e);
        }

        String order() {
            String out = "";
            Entry cur = head;
            while (cur != null) {
                out = out + cur.key + "=" + cur.value + " ";
                cur = cur.next;
            }
            return out.trim();
        }
    }

    public static void main(String[] args) {
        Lru cache = new Lru(3);
        cache.put("a", 1);
        cache.put("b", 2);
        cache.put("c", 3);
        System.out.println(cache.order());
        System.out.println(cache.get("a"));
        cache.put("d", 4);
        System.out.println(cache.order());
        System.out.println(cache.get("b"));
        cache.put("c", 30);
        cache.put("e", 5);
        System.out.println(cache.order());
        System.out.println(cache.get("c"));
        System.out.println(cache.get("zz"));
        System.out.println("hits " + cache.hits + " misses " + cache.misses);
    }
}
