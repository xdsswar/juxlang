import java.util.ArrayList;

public class Main {
    static class Job implements Comparable<Job> {
        String name;
        int priority;
        int arrival;
        int remaining;

        Job(String name, int priority, int arrival, int work) {
            this.name = name;
            this.priority = priority;
            this.arrival = arrival;
            this.remaining = work;
        }

        public int compareTo(Job other) {
            if (priority != other.priority) {
                return Integer.compare(priority, other.priority);
            }
            return Integer.compare(arrival, other.arrival);
        }
    }

    static class Heap {
        private ArrayList<Job> items = new ArrayList<>();

        int size() { return items.size(); }

        void push(Job j) {
            items.add(j);
            int i = items.size() - 1;
            while (i > 0) {
                int parent = (i - 1) / 2;
                if (items.get(i).compareTo(items.get(parent)) < 0) {
                    swap(i, parent);
                    i = parent;
                } else {
                    break;
                }
            }
        }

        Job pop() {
            Job top = items.get(0);
            int last = items.size() - 1;
            items.set(0, items.get(last));
            items.remove(last);
            int i = 0;
            while (true) {
                int l = 2 * i + 1;
                int r = l + 1;
                int smallest = i;
                if (l < items.size() && items.get(l).compareTo(items.get(smallest)) < 0) { smallest = l; }
                if (r < items.size() && items.get(r).compareTo(items.get(smallest)) < 0) { smallest = r; }
                if (smallest == i) { break; }
                swap(i, smallest);
                i = smallest;
            }
            return top;
        }

        private void swap(int a, int b) {
            Job t = items.get(a);
            items.set(a, items.get(b));
            items.set(b, t);
        }
    }

    public static void main(String[] args) {
        Heap heap = new Heap();
        int clock = 0;
        int arrivals = 0;
        heap.push(new Job("backup", 5, arrivals++, 7));
        heap.push(new Job("email", 2, arrivals++, 2));
        heap.push(new Job("render", 3, arrivals++, 5));
        heap.push(new Job("alert", 1, arrivals++, 1));
        heap.push(new Job("index", 3, arrivals++, 3));
        final int slice = 3;
        ArrayList<String> finished = new ArrayList<>();
        while (heap.size() > 0) {
            Job job = heap.pop();
            int run = job.remaining < slice ? job.remaining : slice;
            clock += run;
            job.remaining -= run;
            if (job.remaining > 0) {
                System.out.println("t=" + clock + " " + job.name + " ran " + run + ", " + job.remaining + " left");
                job.priority++;
                job.arrival = arrivals++;
                heap.push(job);
            } else {
                System.out.println("t=" + clock + " " + job.name + " done");
                finished.add(job.name);
                if (job.name.equals("alert")) {
                    heap.push(new Job("page-oncall", 1, arrivals++, 2));
                }
            }
        }
        String order = "";
        for (String n : finished) {
            order += n + " ";
        }
        System.out.println("order: " + order.trim());
        System.out.println("clock: " + clock);
    }
}
