import java.util.ArrayList;

public class Main {
    static class Task implements Comparable<Task> {
        String name;
        int priority;
        int seq;

        Task(String name, int priority, int seq) {
            this.name = name;
            this.priority = priority;
            this.seq = seq;
        }

        public int compareTo(Task other) {
            if (priority != other.priority) { return priority - other.priority; }
            return seq - other.seq;
        }
    }

    static class TaskHeap {
        private ArrayList<Task> items = new ArrayList<>();
        private int nextSeq = 0;

        void add(String name, int priority) {
            items.add(new Task(name, priority, nextSeq));
            nextSeq = nextSeq + 1;
            int i = items.size() - 1;
            while (i > 0) {
                int parent = (i - 1) / 2;
                if (items.get(i).compareTo(items.get(parent)) < 0) {
                    Task t = items.get(i);
                    items.set(i, items.get(parent));
                    items.set(parent, t);
                    i = parent;
                } else {
                    break;
                }
            }
        }

        boolean isEmpty() { return items.size() == 0; }
        int size() { return items.size(); }

        Task poll() {
            Task top = items.get(0);
            Task last = items.remove(items.size() - 1);
            if (items.size() > 0) {
                items.set(0, last);
                int i = 0;
                while (true) {
                    int l = 2 * i + 1;
                    int r = l + 1;
                    int smallest = i;
                    if (l < items.size() && items.get(l).compareTo(items.get(smallest)) < 0) { smallest = l; }
                    if (r < items.size() && items.get(r).compareTo(items.get(smallest)) < 0) { smallest = r; }
                    if (smallest == i) { break; }
                    Task t = items.get(i);
                    items.set(i, items.get(smallest));
                    items.set(smallest, t);
                    i = smallest;
                }
            }
            return top;
        }
    }

    public static void main(String[] args) {
        TaskHeap h = new TaskHeap();
        h.add("write report", 3);
        h.add("fix outage", 1);
        h.add("lunch", 5);
        h.add("review pr", 2);
        h.add("reply email", 3);
        h.add("deploy", 1);
        System.out.println(h.size());
        while (!h.isEmpty()) {
            Task t = h.poll();
            System.out.println(t.priority + " " + t.name);
        }
        h.add("late", 4);
        h.add("early", 0);
        System.out.println(h.poll().name);
    }
}
