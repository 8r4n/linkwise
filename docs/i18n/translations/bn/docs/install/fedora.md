# Fedora-এর জন্য Linkwise মডেম প্রিভিউ

Linkwise দ্বিমুখী সফটওয়্যার মডেম, স্টেশন ডায়াগনস্টিক ও স্থানীয় সিমুলেশন দেয়। অডিও রাউটিং ও প্রেরণের সেটিংয়ের জন্য Station Setup দেখুন।

প্রিভিউ ইনস্টলারে `secure-transfer` নেই। gzip/SHA-256 ফাইল স্থানান্তর, যৌথ PKI, সুরক্ষিত চ্যাট ও মেল এবং সার্টিফিকেটসহ QSL কার্ডের জন্য সোর্স থেকে বিল্ড করুন। কনফিগারেশন ও হাইব্রিড চ্যাটের শর্ত [PKI নির্দেশিকায়](../../../../../../rust/docs/pki.md) আছে।

## ইনস্টল বা আপডেট

আর্কিটেকচার জানতে `uname -m` চালান। ডাউনলোড ডিরেক্টরিতে নিচের কমান্ডগুলো চালিয়ে উদাহরণের বদলে মেলে এমন প্যাকেজের ফাইলের নাম ব্যবহার করুন।

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF প্রয়োজনীয় সিস্টেম লাইব্রেরি ইনস্টল করে। আপডেটে নতুন ফাইল দিয়ে `sudo dnf upgrade ./FILE.rpm` চালান। সিমুলেশন স্থানীয়ভাবে রেডিও ছাড়া চলে।

## স্টেশন সেট আপ করুন

কমান্ডের জন্য `man 8 linkwise-modemd` এবং বিন্যাসের জন্য `man 5 linkwise-station` দেখুন। ডিভাইস নির্বাচন ও `station.json` তৈরির জন্য `/usr/share/doc/linkwise-modem/Station-setup.md` খুলুন। বর্তমান অডিও সেশনে প্রবেশের জন্য ডেস্কটপ ব্যবহারকারী হিসেবে পরীক্ষা চালান।

## মডেম সেবা চালান

ডেস্কটপ স্টেশনের কনফিগারেশন `~/.config/linkwise/station.json`-এ সেভ করে নিচের কমান্ড চালান।

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

নির্দিষ্ট স্টেশনের জন্য প্রশাসকের মালিকানাধীন কনফিগারেশন ও প্রোফাইল `/etc/linkwise`-এ রাখুন। সেবাকে ফাইল পড়তে এবং কনফিগার করা ALSA ইনপুট ও আউটপুট সরাসরি ব্যবহার করতে দিতে হবে। একই ডিভাইস ও পোর্ট জোড়ার জন্য একটি সেবা চালান।

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

`bandwidth` দিলে মডেম IPv4 লুপব্যাকে দুই পোর্ট খোলে। ক্ষেত্রটি না থাকলে কমান্ড পোর্ট JSON-এ রিসিভ ডায়াগনস্টিক দেয়। `nc 127.0.0.1 8300` দিয়ে পড়ুন।

### SELinux

SELinux enforcing রাখুন। RPM সিস্টেম সেবার নীতি ইনস্টল করে। কনফিগারেশন তৈরি বা বদলের পর লেবেল ফিরিয়ে সক্রিয় ডোমেইন যাচাই করুন।

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

এক্সিকিউটেবলে `linkwise_modem_exec_t`, কনফিগারেশনে `linkwise_modem_etc_t` এবং সিস্টেম সেবায় `linkwise_modem_t` প্রত্যাশিত। ডিফল্ট TCP 8300 ও 8301-এর ধরন `linkwise_modem_port_t`। অন্য পোর্টে এই ধরন দিন ও লেবেল দ্বন্দ্ব মেটান। ডেস্কটপ সেবা চালানো ব্যবহারকারীর ডোমেইনে চলে।

Fedora-তে SELinux প্রয়োগ পরীক্ষা ও DISA STIG যোগ্যতা আলাদা। স্থাপনের আগে STIG-এর আওতাভুক্ত OS ইমেজ, নীতির ধরন ও সাইটের বেসলাইন যাচাই করুন।

## আনইনস্টল

চলমান Linkwise কমান্ড বন্ধ করে `sudo dnf remove linkwise-modem` চালান। হোম ডিরেক্টরির স্টেশন ফাইলগুলো থাকবে।
